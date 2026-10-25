import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { IMAGERY_SOURCES } from '../imagery.ts';
import { IMAGERIES, type Imagery } from '../theme.ts';

export type Tile = { readonly status: number; readonly body: Buffer; readonly contentType: string };

export type TileOptions = {
  /** Dossier du cache : `<dossier>/<imagerie>/<z>/<x>/<y>`. */
  readonly cacheDir: string;
  /** Essais par tuile, le premier compris. */
  readonly attempts: number;
  /** Attente avant le deuxième essai, doublée à chaque suivant. */
  readonly retryDelayMs: number;
  /** Requêtes simultanées vers les fournisseurs, tous serveurs confondus. */
  readonly concurrency: number;
};

export const DEFAULT_TILE_OPTIONS: TileOptions = {
  cacheDir: 'data/cache/tiles',
  attempts: 6,
  retryDelayMs: 500,
  // EOX coupe des connexions au-delà de quelques dizaines de requêtes simultanées.
  concurrency: 8,
};

const HEADERS = { 'user-agent': 'dijkstravel/0.1 (outil personnel de cartographie)' };

/** `/imagery/<nom>/<z>/<x>/<y>` → sa tuile ; `null` pour tout autre chemin. */
export function parseTilePath(
  pathname: string,
): { readonly imagery: Imagery; readonly z: number; readonly x: number; readonly y: number } | null {
  const match = /^\/imagery\/([a-z]+)\/(\d+)\/(\d+)\/(\d+)$/.exec(pathname);
  if (match === null) return null;
  const [, name = '', z = '', x = '', y = ''] = match;
  const imagery = IMAGERIES.find((known) => known === name);
  return imagery === undefined ? null : { imagery, z: Number(z), x: Number(x), y: Number(y) };
}

/** File d'attente qui laisse passer `limit` tâches à la fois. */
export function limiter(limit: number): <T>(task: () => Promise<T>) => Promise<T> {
  let running = 0;
  const waiting: (() => void)[] = [];
  return async (task) => {
    if (running >= limit) await new Promise<void>((resolve) => waiting.push(resolve));
    running++;
    try {
      return await task();
    } finally {
      running--;
      waiting.shift()?.();
    }
  };
}

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** Réessayé : connexion coupée, trop de requêtes, erreur du serveur. Pas un 404 (hors couverture). */
const retryable = (status: number): boolean => status === 429 || status >= 500;

/**
 * Tuiles d'imagerie, servies par le serveur de rendu au lieu du fournisseur : une tuile en échec
 * (connexion coupée sous la charge des workers) serait comblée par MapLibre avec celle du zoom
 * inférieur, floue, et la jonction se verrait sur la vidéo. Chaque tuile obtenue est gardée sur
 * disque : les rendus suivants n'interrogent plus le fournisseur.
 */
export function tileFetcher(options: TileOptions = DEFAULT_TILE_OPTIONS) {
  const queue = limiter(options.concurrency);
  return async (imagery: Imagery, z: number, x: number, y: number): Promise<Tile> => {
    const file = join(options.cacheDir, imagery, String(z), String(x), String(y));
    const cached = await readFile(file).catch(() => null);
    if (cached !== null) return { status: 200, body: cached, contentType: contentTypeOf(cached) };
    const url = IMAGERY_SOURCES[imagery].tiles
      .replace('{z}', String(z))
      .replace('{x}', String(x))
      .replace('{y}', String(y));
    let last: Tile = { status: 502, body: Buffer.alloc(0), contentType: 'text/plain' };
    for (let attempt = 0; attempt < options.attempts; attempt++) {
      if (attempt > 0) await sleep(options.retryDelayMs * 2 ** (attempt - 1));
      const response = await queue(() => fetch(url, { headers: HEADERS })).catch(() => null);
      if (response === null) continue;
      const body = Buffer.from(await response.arrayBuffer());
      last = { status: response.status, body, contentType: response.headers.get('content-type') ?? 'image/jpeg' };
      if (response.ok) {
        // Écrite à côté puis renommée : un rendu concurrent ne lit jamais une tuile à moitié écrite.
        await mkdir(dirname(file), { recursive: true });
        const partial = `${file}.${String(process.pid)}.part`;
        await writeFile(partial, body);
        await rename(partial, file);
        return last;
      }
      if (!retryable(response.status)) return last;
    }
    return last;
  };
}

/** Type d'après les premiers octets : le cache ne garde que les tuiles. */
function contentTypeOf(body: Buffer): string {
  return body[0] === 0x89 && body[1] === 0x50 ? 'image/png' : 'image/jpeg';
}
