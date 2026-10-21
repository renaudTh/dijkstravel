import { createReadStream } from 'node:fs';
import { readFile, stat } from 'node:fs/promises';
import { createServer } from 'node:http';
import { stripTypeScriptTypes } from 'node:module';
import { extname, join, normalize, sep } from 'node:path';

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
};

/** Seuls `src/` et la distribution de maplibre-gl sont servis : jamais `data/` ni le reste du disque. */
function resolvePublic(pathname: string): string | null {
  const file = normalize(join(ROOT, decodeURIComponent(pathname)));
  const allowed = [join(ROOT, 'src') + sep, join(ROOT, 'node_modules', 'maplibre-gl', 'dist') + sep];
  return allowed.some((prefix) => file.startsWith(prefix)) ? file : null;
}

/**
 * Sert `src/render/index.html`, les modules `.ts` du projet débarrassés de leurs types
 * (`module.stripTypeScriptTypes`), maplibre-gl depuis node_modules, et le run et la tournée
 * sous `/run.json` et `/tour.json`.
 */
export function servePage(files: { readonly run: string; readonly tour: string }): Promise<PageServer> {
  const aliases: Readonly<Record<string, string>> = {
    '/': join(ROOT, 'src', 'render', 'index.html'),
    '/run.json': files.run,
    '/tour.json': files.tour,
  };
  const server = createServer((request, response) => {
    const pathname = new URL(request.url ?? '/', 'http://localhost').pathname;
    const file = aliases[pathname] ?? resolvePublic(pathname);
    const send = async (): Promise<void> => {
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
