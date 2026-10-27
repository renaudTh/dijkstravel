import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';

export type Tile = { readonly status: number; readonly body: Buffer; readonly contentType: string };

export type TileOptions = {
  /** Dossier du cache : `<dossier>/<source>/<z>/<x>/<y>`. */
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
 * Tuiles d'une source quelconque (imagerie, relief, tuiles vectorielles), téléchargées avant le rendu,
 * réessayées (connexion coupée, serveur surchargé) et gardées sous `<cacheDir>/<name>/<z>/<x>/<y>` :
 * le moteur ne lit que le disque, et les rendus suivants n'interrogent plus le fournisseur. `template` (`{z}`, `{x}`, `{y}`) n'est demandé que pour une tuile
 * absente du cache : une adresse qui se lit sur le réseau (TileJSON) n'est pas requise hors ligne.
 */
export function sourceFetcher(options: TileOptions = DEFAULT_TILE_OPTIONS) {
  const queue = limiter(options.concurrency);
  return async (name: string, template: () => Promise<string>, z: number, x: number, y: number): Promise<Tile> => {
    const file = join(options.cacheDir, name, String(z), String(x), String(y));
    const cached = await readFile(file).catch(() => null);
    if (cached !== null) return { status: 200, body: cached, contentType: contentTypeOf(cached) };
    const url = (await template()).replace('{z}', String(z)).replace('{x}', String(x)).replace('{y}', String(y));
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
