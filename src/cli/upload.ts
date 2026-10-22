import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { basename } from 'node:path';
import { request as httpRequest } from 'node:http';
import { request as httpsRequest } from 'node:https';
import { runPaths } from '../run.ts';
import { loadRun, usage } from './shared.ts';

/** Dossier Nextcloud qui reçoit un sous-dossier par run. */
const REMOTE_ROOT = ['Documents', 'dijkstravel'];

class NextcloudError extends Error {
  readonly status: number;

  constructor(what: string, status: number) {
    super(`Nextcloud : ${what} (HTTP ${String(status)})`);
    this.status = status;
  }
}

type Connection = { readonly davRoot: string; readonly authorization: string };

/** Seul lecteur des variables Nextcloud, chargées depuis `.env` par `npm run upload`. */
function connection(): Connection | null {
  const url = process.env['NEXTCLOUD_URL'];
  const user = process.env['NEXTCLOUD_USER'];
  const password = process.env['NEXTCLOUD_APP_PASSWORD'];
  if (!url || !user || !password) return null;
  return {
    davRoot: `${url.replace(/\/+$/, '')}/remote.php/dav/files/${encodeURIComponent(user)}`,
    authorization: `Basic ${Buffer.from(`${user}:${password}`).toString('base64')}`,
  };
}

/** Crée chaque dossier du chemin ; 405 veut dire « existe déjà ». */
async function ensureFolders(nc: Connection, segments: readonly string[]): Promise<string> {
  let path = nc.davRoot;
  for (const segment of segments) {
    path += `/${encodeURIComponent(segment)}`;
    // Séquentiel : un dossier n'existe qu'une fois son parent créé.
    const response = await fetch(path, { method: 'MKCOL', headers: { authorization: nc.authorization } });
    if (response.status !== 201 && response.status !== 405)
      throw new NextcloudError(`création de ${segment}`, response.status);
  }
  return path;
}

/** Envoi en flux : une vidéo de 200 Mo ne passe pas par la mémoire. */
async function upload(nc: Connection, folder: string, file: string): Promise<number> {
  const { size } = await stat(file);
  const url = new URL(`${folder}/${encodeURIComponent(basename(file))}`);
  const send = url.protocol === 'https:' ? httpsRequest : httpRequest;
  await new Promise<void>((resolve, reject) => {
    const put = send(
      url,
      { method: 'PUT', headers: { authorization: nc.authorization, 'content-length': String(size) } },
      (response) => {
        response.resume();
        const status = response.statusCode ?? 0;
        if (status >= 200 && status < 300) resolve();
        else reject(new NextcloudError(`envoi de ${basename(file)}`, status));
      },
    );
    put.on('error', reject);
    createReadStream(file).on('error', reject).pipe(put);
  });
  return size;
}

/**
 * `npm run upload -- <run.json>` : envoie la vidéo, la couverture et `publication.txt` du run dans
 * `Documents/dijkstravel/<run>/` du Nextcloud décrit dans `.env` (fichiers déjà présents remplacés).
 */
export async function main(argv: readonly string[]): Promise<number> {
  const [runFile] = argv;
  if (runFile === undefined) return usage('upload <run.json>');
  const nc = connection();
  if (nc === null) {
    console.error('NEXTCLOUD_URL, NEXTCLOUD_USER et NEXTCLOUD_APP_PASSWORD attendus dans .env');
    return 1;
  }
  const run = await loadRun(runFile);
  if (run === null) return 1;
  const paths = runPaths(run);
  const candidates = [paths.video, `${paths.dir}/cover.jpg`, `${paths.dir}/publication.txt`];
  const present = await Promise.all(
    candidates.map((file) =>
      stat(file).then(
        () => file,
        () => null,
      ),
    ),
  );
  const files = present.filter((file): file is string => file !== null);
  if (!files.includes(paths.video)) {
    console.error(`${paths.video} absente : rendre la vidéo d'abord (npm run render -- ${runFile}).`);
    return 1;
  }
  const folder = await ensureFolders(nc, [...REMOTE_ROOT, run.name]);
  // Séquentiel : un fichier à la fois sur la bande passante.
  for (const file of files) {
    const size = await upload(nc, folder, file);
    console.log(`${file} → ${[...REMOTE_ROOT, run.name, basename(file)].join('/')} (${(size / 1e6).toFixed(1)} Mo)`);
  }
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
