import { createReadStream } from 'node:fs';
import { readdir, readFile, stat } from 'node:fs/promises';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { stripTypeScriptTypes } from 'node:module';
import { basename, dirname, extname, join, normalize, sep } from 'node:path';
import { writeJson } from '../files.ts';
import { parseTheme } from '../theme.ts';
import { DEFAULT_TILE_OPTIONS, parseTilePath, tileFetcher } from './tiles.ts';

export type PageServer = {
  readonly url: string;
  readonly close: () => Promise<void>;
};

const ROOT = join(import.meta.dirname, '..', '..');

const CONTENT_TYPES: Readonly<Record<string, string>> = {
  '.html': 'text/html; charset=utf-8',
  '.ts': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.map': 'application/json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
};

const IMAGE_EXTENSIONS = new Set(['.svg', '.png', '.jpg', '.jpeg', '.webp']);

/** Seuls `src/` et la distribution de maplibre-gl sont servis : jamais `data/` ni le reste du disque. */
function resolvePublic(pathname: string): string | null {
  const file = normalize(join(ROOT, decodeURIComponent(pathname)));
  const allowed = [join(ROOT, 'src') + sep, join(ROOT, 'node_modules', 'maplibre-gl', 'dist') + sep];
  return allowed.some((prefix) => file.startsWith(prefix)) ? file : null;
}

/** Un thème ne pèse que quelques centaines d'octets : tout corps plus gros est refusé. */
const MAX_THEME_BYTES = 64 * 1024;

async function readBody(request: IncomingMessage): Promise<string | null> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk));
    size += buffer.length;
    if (size > MAX_THEME_BYTES) return null;
    chunks.push(buffer);
  }
  return Buffer.concat(chunks).toString('utf8');
}

/** Enregistre le thème envoyé par l'éditeur, après validation : jamais de fichier écrit tel quel. */
async function saveTheme(request: IncomingMessage, response: ServerResponse, themeFile: string): Promise<void> {
  const body = await readBody(request);
  let raw: unknown = null;
  try {
    raw = body === null ? null : JSON.parse(body);
  } catch {
    raw = null;
  }
  const theme = parseTheme(raw);
  if (!theme.ok) {
    response.writeHead(400, { 'content-type': 'application/json' }).end(JSON.stringify(theme.errors));
    return;
  }
  await writeJson(themeFile, theme.value);
  response.writeHead(204).end();
}

/**
 * Sert `src/render/index.html` (rendu) et `src/render/editor.html` (éditeur de thème), les modules `.ts`
 * du projet débarrassés de leurs types (`module.stripTypeScriptTypes`), maplibre-gl depuis node_modules,
 * le run, la tournée et le thème sous `/run.json`, `/tour.json` et `/theme.json`, et les images
 * des lieux (dossier `images/` à côté de la tournée) sous `/images.json` et `/images/<fichier>`, et les
 * tuiles d'imagerie sous `/imagery/<nom>/<z>/<x>/<y>` (cache `data/cache/tiles/`, voir `tileFetcher`).
 * `editable` : accepte `PUT /theme.json`, qui écrit le thème validé dans `files.theme`.
 */
export function servePage(files: {
  readonly run: string;
  readonly tour: string;
  readonly theme: string;
  readonly editable: boolean;
}): Promise<PageServer> {
  const aliases: Readonly<Record<string, string>> = {
    '/': join(ROOT, 'src', 'render', 'index.html'),
    '/editor': join(ROOT, 'src', 'render', 'editor.html'),
    '/run.json': files.run,
    '/tour.json': files.tour,
  };
  const imagesDir = join(dirname(files.tour), 'images');
  const fetchTile = tileFetcher({ ...DEFAULT_TILE_OPTIONS, cacheDir: join(ROOT, 'data', 'cache', 'tiles') });
  const server = createServer((request, response) => {
    const pathname = new URL(request.url ?? '/', 'http://localhost').pathname;
    const send = async (): Promise<void> => {
      if (pathname === '/theme.json' && request.method === 'PUT' && files.editable) {
        await saveTheme(request, response, files.theme);
        return;
      }
      const tile = parseTilePath(pathname);
      if (tile !== null) {
        const { status, body, contentType } = await fetchTile(tile.imagery, tile.z, tile.x, tile.y);
        response.writeHead(status, { 'content-type': contentType }).end(body);
        return;
      }
      if (pathname === '/images.json') {
        // Images des lieux, rangées à côté de la tournée : `images/<id du lieu>.<ext>`.
        const names = await readdir(imagesDir).catch(() => []);
        const byPlace = Object.fromEntries(
          names
            .filter((name) => IMAGE_EXTENSIONS.has(extname(name).toLowerCase()))
            .map((name) => [basename(name, extname(name)), `/images/${encodeURIComponent(name)}`]),
        );
        response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(byPlace));
        return;
      }
      if (pathname === '/theme.json') {
        // Thème absent : un objet vide, que `parseTheme` complète avec les valeurs par défaut.
        const text = await readFile(files.theme, 'utf8').catch(() => '{}');
        response.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' }).end(text);
        return;
      }
      // `basename` : jamais de sortie du dossier d'images, quel que soit le chemin demandé.
      const image = pathname.startsWith('/images/') ? join(imagesDir, basename(decodeURIComponent(pathname))) : null;
      const file = image ?? aliases[pathname] ?? resolvePublic(pathname);
      if (file === null || !(await stat(file).catch(() => null))?.isFile()) {
        response.writeHead(404).end();
        return;
      }
      response.setHeader('content-type', CONTENT_TYPES[extname(file)] ?? 'application/octet-stream');
      if (extname(file) === '.ts') response.end(stripTypeScriptTypes(await readFile(file, 'utf8')));
      else createReadStream(file).pipe(response);
    };
    send().catch((error: unknown) => {
      console.error(error);
      response.writeHead(500).end();
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      const port = typeof address === 'object' && address !== null ? address.port : 0;
      resolve({
        url: `http://127.0.0.1:${String(port)}/`,
        close: () =>
          new Promise<void>((done) => {
            server.close(() => {
              done();
            });
            server.closeAllConnections();
          }),
      });
    });
  });
}
