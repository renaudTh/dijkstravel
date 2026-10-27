import { spawn } from 'node:child_process';
import { access, mkdir, readdir, readFile, rename, writeFile } from 'node:fs/promises';
import { basename, extname, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { readJson, writeJson } from '../files.ts';
import { isRecord } from '../narrow.ts';
import { IMAGERY_SOURCES, TERRAIN_SOURCE, VECTOR_SOURCE } from '../imagery.ts';
import { runPaths, runTitle, VIDEO_SIZES, type Run } from '../run.ts';
import { parseTour } from '../solve/tour.ts';
import { parseTheme, type Theme } from '../theme.ts';
import { DEFAULT_TILE_OPTIONS, limiter, sourceFetcher } from './tiles.ts';

/**
 * Moteur de rendu natif (`render/`, Rust + Skia, sans GPU) : Node valide le run et le thème, écrit
 * l'entrée du moteur et remplit le cache des tuiles ; le moteur ne lit que le disque.
 */
const ROOT = join(import.meta.dirname, '..', '..');
const BINARY = join(ROOT, 'render', 'target', 'release', 'dijkstravel-render');
const TILE_DIR = join(ROOT, 'data', 'cache', 'tiles');
const FONT_DIR = join(ROOT, 'data', 'cache', 'fonts');
/** Graisses de la carte d'infos : titre (700), texte (800), distance (900). */
const HUD_WEIGHTS = [700, 800, 900] as const;
const IMAGE_EXTENSIONS = new Set(['.svg', '.png', '.jpg', '.jpeg', '.webp']);

export class NativeRenderError extends Error {
  readonly code: number | null;

  constructor(command: string, code: number | null) {
    super(`dijkstravel-render ${command} a échoué (code ${String(code)})`);
    this.code = code;
  }
}

export class MissingBinaryError extends Error {
  constructor() {
    super(`Moteur de rendu absent (${BINARY}) : npm run build:render`);
  }
}

/** Images des lieux, rangées à côté de la tournée : `images/<id du lieu>.<ext>` → chemin absolu. */
async function placeImages(dir: string): Promise<Record<string, string>> {
  const names = await readdir(dir).catch(() => []);
  return Object.fromEntries(
    names
      .filter((name) => IMAGE_EXTENSIONS.has(extname(name).toLowerCase()))
      .map((name) => [basename(name, extname(name)), resolve(dir, name)]),
  );
}

/**
 * Fichiers TTF de la police de la carte d'infos, une par graisse proposée par Google Fonts (une
 * graisse absente est remplacée par la plus proche au rendu). Téléchargés une fois dans `data/cache/fonts/`.
 */
async function hudFonts(family: string): Promise<string[]> {
  const files: string[] = [];
  for (const weight of HUD_WEIGHTS) {
    const file = join(FONT_DIR, `${family.replaceAll(' ', '-')}-${String(weight)}.ttf`);
    const cached = await access(file).then(
      () => true,
      () => false,
    );
    if (cached || (await downloadFont(family, weight, file))) files.push(file);
  }
  if (files.length === 0) throw new Error(`Police ${family} introuvable sur Google Fonts`);
  return files;
}

/** `false` si Google Fonts ne propose pas cette graisse (ou hors ligne). */
async function downloadFont(family: string, weight: number, file: string): Promise<boolean> {
  const query = `${encodeURIComponent(family)}:wght@${String(weight)}`;
  // Ancien agent : Google Fonts répond par des TTF, que Skia charge, au lieu de WOFF2.
  const headers = { 'user-agent': 'Wget/1.21' };
  const css = await fetch(`https://fonts.googleapis.com/css2?family=${query}`, { headers }).catch(() => null);
  if (css === null || !css.ok) return false;
  const url = /src: url\((https:[^)]+\.ttf)\)/.exec(await css.text())?.[1];
  const ttf = url === undefined ? null : await fetch(url).catch(() => null);
  if (ttf === null || !ttf.ok) return false;
  await mkdir(FONT_DIR, { recursive: true });
  const partial = `${file}.${String(process.pid)}.part`;
  await writeFile(partial, Buffer.from(await ttf.arrayBuffer()));
  await rename(partial, file);
  return true;
}

/** Thème d'un run ; absent, un objet vide que `parseTheme` complète avec les valeurs par défaut. */
export async function loadTheme(themeFile: string): Promise<Theme> {
  const theme = parseTheme(JSON.parse(await readFile(themeFile, 'utf8').catch(() => '{}')));
  if (!theme.ok) throw new Error(`${themeFile} : thème invalide`);
  return theme.value;
}

/**
 * Écrit l'entrée du moteur pour un thème (`data/<run>/render-input.json` par défaut) et renvoie son
 * chemin. La tournée, le titre et les images ne sont lus qu'une fois : l'éditeur en écrit une par aperçu.
 */
export async function inputWriter(
  runFile: string,
  run: Run,
): Promise<(theme: Theme, file?: string) => Promise<string>> {
  const paths = runPaths(run);
  // Lu ici pour échouer tôt ; le moteur relit le même fichier.
  const tour = parseTour(await readJson(paths.tour));
  const title = runTitle(await readJson(runFile)) ?? `${String(tour.stops.length)} lieux · ${run.name}`;
  const images = await placeImages(join(paths.dir, 'images'));
  return async (theme, file = `${paths.dir}/render-input.json`) => {
    const imagery = IMAGERY_SOURCES[theme.basemap.imagery];
    await writeJson(file, {
      video: { ...run.video, ...VIDEO_SIZES[run.video.format] },
      theme,
      tourFile: resolve(paths.tour),
      title,
      images,
      imagery: { name: theme.basemap.imagery, maxzoom: imagery.maxzoom, credit: imagery.credit },
      terrain: { name: TERRAIN_SOURCE.name, maxzoom: TERRAIN_SOURCE.maxzoom },
      vector: { name: VECTOR_SOURCE.name, maxzoom: VECTOR_SOURCE.maxzoom },
      tileDir: TILE_DIR,
      hudFonts: await hudFonts(theme.hud.font),
    });
    return file;
  };
}

/** `data/<run>/render-input.json` pour le thème enregistré du run. */
export async function writeInput(runFile: string, run: Run, themeFile: string): Promise<string> {
  const write = await inputWriter(runFile, run);
  return write(await loadTheme(themeFile));
}

async function binary(): Promise<string> {
  await access(BINARY).catch(() => {
    throw new MissingBinaryError();
  });
  return BINARY;
}

/** Lance le moteur ; `onLine` reçoit chaque ligne de sa sortie standard. */
async function runEngine(args: readonly string[], onLine: (line: string) => void = () => undefined): Promise<void> {
  const engine = spawn(await binary(), args, { stdio: ['ignore', 'pipe', 'inherit'] });
  const lines = createInterface({ input: engine.stdout });
  lines.on('line', onLine);
  const code = await new Promise<number | null>((done) => {
    engine.on('close', done);
  });
  if (code !== 0) throw new NativeRenderError(args[0] ?? '', code);
}

/** Adresse des tuiles vectorielles, lue une fois dans le TileJSON et seulement s'il en manque une. */
function vectorTemplate(): () => Promise<string> {
  let template: Promise<string> | null = null;
  return () => {
    template ??= fetch(VECTOR_SOURCE.tilejson)
      .then((response) => response.json())
      .then((raw: unknown) => {
        const list: unknown = isRecord(raw) ? raw['tiles'] : undefined;
        const tiles: unknown = Array.isArray(list) ? list[0] : undefined;
        if (typeof tiles !== 'string') throw new Error(`${VECTOR_SOURCE.tilejson} : pas d'adresse de tuiles`);
        return tiles;
      });
    return template;
  };
}

/** Adresse des tuiles de chaque source que le moteur peut demander. */
function templates(): ReadonlyMap<string, () => Promise<string>> {
  const fixed = (url: string) => () => Promise.resolve(url);
  return new Map([
    ...Object.entries(IMAGERY_SOURCES).map(([name, source]) => [name, fixed(source.tiles)] as const),
    [TERRAIN_SOURCE.name, fixed(TERRAIN_SOURCE.tiles)],
    [VECTOR_SOURCE.name, vectorTemplate()],
  ]);
}

/**
 * Télécharge dans le cache toutes les tuiles que liront les images choisies (`--frames=<n>` ou
 * `--frame=<n>`, toute la vidéo sinon).
 */
export async function prefetchTiles(input: string, which?: string): Promise<void> {
  const tiles: string[] = [];
  await runEngine(['tiles', input, ...(which === undefined ? [] : [which])], (line) => tiles.push(line));
  await fetchTiles(tiles, false);
}

/** Tuiles `<source>/<z>/<x>/<y>` absentes du cache, téléchargées. */
async function fetchTiles(tiles: readonly string[], quiet: boolean): Promise<void> {
  const fetchTile = sourceFetcher({ ...DEFAULT_TILE_OPTIONS, cacheDir: TILE_DIR });
  const sources = templates();
  let done = 0;
  await Promise.all(
    tiles.map(async (line) => {
      const [name = '', z, x, y] = line.split('/');
      const template = sources.get(name);
      if (template === undefined) throw new Error(`Source de tuiles inconnue : ${line}`);
      // Le chemin dans le cache est celui de la ligne : une tuile déjà là n'est même pas lue.
      const cached = await access(join(TILE_DIR, line)).then(
        () => true,
        () => false,
      );
      if (!cached) await fetchTile(name, template, Number(z), Number(x), Number(y));
      done++;
      if (!quiet && (done % 50 === 0 || done === tiles.length)) {
        process.stdout.write(`\rtuiles ${String(done)} / ${String(tiles.length)}`);
      }
    }),
  );
  if (!quiet && tiles.length > 0) process.stdout.write('\n');
}

/** Une image en PNG ou JPEG (d'après l'extension de `outFile`) ; `hud` : avec la carte d'infos. */
export async function renderFrame(input: string, frame: number, outFile: string, hud = true): Promise<void> {
  await runEngine(['frame', input, `--frame=${String(frame)}`, `--out=${outFile}`, ...(hud ? [] : ['--no-hud'])]);
}

/** La vidéo, ou ses `frames` premières images ; `threads` : threads de rendu (tous les cœurs par défaut). */
export async function renderVideo(
  input: string,
  outFile: string,
  options: { readonly frames?: number; readonly threads?: number } = {},
): Promise<void> {
  await runEngine([
    'video',
    input,
    `--out=${outFile}`,
    ...(options.frames === undefined ? [] : [`--frames=${String(options.frames)}`]),
    ...(options.threads === undefined ? [] : [`--threads=${String(options.threads)}`]),
  ]);
}

export type PreviewEngine = {
  /** Image `frame` de l'entrée `input` dans `outFile` (PNG ou JPEG d'après l'extension), tuiles téléchargées au besoin. */
  readonly render: (input: string, frame: number, outFile: string) => Promise<void>;
  readonly close: () => void;
};

/**
 * Moteur gardé en vie pour l'éditeur (`dijkstravel-render serve`) : la tournée et l'atlas restent en
 * mémoire, un réglage de couleur ne recalcule que ce qui en dépend. Une requête à la fois.
 */
export function previewEngine(): PreviewEngine {
  const queue = limiter(1);
  let engine: Promise<{ readonly ask: (request: string) => Promise<string>; readonly kill: () => void }> | null = null;
  const start = async () => {
    const child = spawn(await binary(), ['serve'], { stdio: ['pipe', 'pipe', 'inherit'] });
    const waiting: ((line: string) => void)[] = [];
    createInterface({ input: child.stdout }).on('line', (line) => waiting.shift()?.(line));
    child.on('close', () => {
      engine = null;
      for (const reply of waiting.splice(0)) reply('erreur moteur arrêté');
    });
    return {
      ask: (request: string) =>
        new Promise<string>((reply) => {
          waiting.push(reply);
          child.stdin.write(`${request}\n`);
        }),
      kill: () => child.kill(),
    };
  };
  return {
    render: (input, frame, outFile) =>
      queue(async () => {
        engine ??= start();
        const running = await engine;
        const listed = await running.ask(JSON.stringify({ input: resolve(input), frame }));
        if (!listed.startsWith('tuiles')) throw new Error(listed);
        await fetchTiles(listed.split(' ').slice(1).filter(Boolean), true);
        const reply = await running.ask(JSON.stringify({ input: resolve(input), frame, out: resolve(outFile) }));
        if (reply !== 'ok') throw new Error(reply);
      }),
    close: () => {
      void engine?.then((running) => {
        running.kill();
      });
    },
  };
}
