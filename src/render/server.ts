import { readFile, stat } from 'node:fs/promises';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { stripTypeScriptTypes } from 'node:module';
import { extname, join, normalize, sep } from 'node:path';
import { writeJson } from '../files.ts';
import { isRecord } from '../narrow.ts';
import { runPaths, type Run } from '../run.ts';
import { parseTheme } from '../theme.ts';
import { inputWriter, previewEngine } from './native.ts';

export type EditorServer = {
  readonly url: string;
  readonly close: () => Promise<void>;
};

const ROOT = join(import.meta.dirname, '..', '..');

const CONTENT_TYPES: Readonly<Record<string, string>> = {
  '.html': 'text/html; charset=utf-8',
  '.ts': 'text/javascript; charset=utf-8',
  '.json': 'application/json',
};

/** Seul `src/` est servi : jamais `data/` ni le reste du disque. */
function resolvePublic(pathname: string): string | null {
  const file = normalize(join(ROOT, decodeURIComponent(pathname)));
  return file.startsWith(join(ROOT, 'src') + sep) ? file : null;
}

/** Un thème (ou une demande d'aperçu) ne pèse que quelques centaines d'octets : tout corps plus gros est refusé. */
const MAX_BODY_BYTES = 64 * 1024;

async function readBody(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk));
    size += buffer.length;
    if (size > MAX_BODY_BYTES) return null;
    chunks.push(buffer);
  }
  try {
    const raw: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    return raw;
  } catch {
    return null;
  }
}

/** Enregistre le thème envoyé par l'éditeur, après validation : jamais de fichier écrit tel quel. */
async function saveTheme(request: IncomingMessage, response: ServerResponse, themeFile: string): Promise<void> {
  const theme = parseTheme(await readBody(request));
  if (!theme.ok) {
    response.writeHead(400, { 'content-type': 'application/json' }).end(JSON.stringify(theme.errors));
    return;
  }
  await writeJson(themeFile, theme.value);
  response.writeHead(204).end();
}

/**
 * Éditeur de thème : sert `src/render/editor.html`, les modules `.ts` du projet débarrassés de leurs types
 * (`module.stripTypeScriptTypes`), le run et le thème sous `/run.json` et `/theme.json` (`PUT` : thème
 * validé écrit dans `files.theme`), et l'aperçu : `POST /preview` (`{ theme, frame }`) répond l'image
 * en JPEG, rendue par le moteur natif gardé en vie (`previewEngine`), comme la vidéo la rendra.
 */
export async function serveEditor(files: {
  readonly runFile: string;
  readonly run: Run;
  readonly theme: string;
}): Promise<EditorServer> {
  const aliases: Readonly<Record<string, string>> = {
    '/': join(ROOT, 'src', 'render', 'editor.html'),
    '/editor': join(ROOT, 'src', 'render', 'editor.html'),
    '/run.json': files.runFile,
  };
  const dir = runPaths(files.run).dir;
  const writeInput = await inputWriter(files.runFile, files.run);
  const engine = previewEngine();

  const preview = async (request: IncomingMessage, response: ServerResponse): Promise<void> => {
    const body = await readBody(request);
    const theme = parseTheme(isRecord(body) ? body['theme'] : null);
    const frame = isRecord(body) ? body['frame'] : null;
    if (!theme.ok || typeof frame !== 'number' || !Number.isInteger(frame) || frame < 0) {
      const issues = theme.ok ? [{ path: 'frame', expected: 'entier >= 0' }] : theme.errors;
      response.writeHead(400, { 'content-type': 'application/json' }).end(JSON.stringify(issues));
      return;
    }
    const input = await writeInput(theme.value, `${dir}/render-input.preview.json`);
    // JPEG : bien plus rapide à encoder qu'un PNG de cette taille, sans différence visible à l'écran.
    const image = `${dir}/preview.jpg`;
    await engine.render(input, frame, image);
    response.writeHead(200, { 'content-type': 'image/jpeg', 'cache-control': 'no-store' }).end(await readFile(image));
  };

  const server = createServer((request, response) => {
    const pathname = new URL(request.url ?? '/', 'http://localhost').pathname;
    const send = async (): Promise<void> => {
      if (pathname === '/preview' && request.method === 'POST') {
        await preview(request, response);
        return;
      }
      if (pathname === '/theme.json' && request.method === 'PUT') {
        await saveTheme(request, response, files.theme);
        return;
      }
      if (pathname === '/theme.json') {
        // Thème absent : un objet vide, que `parseTheme` complète avec les valeurs par défaut.
        const text = await readFile(files.theme, 'utf8').catch(() => '{}');
        response.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' }).end(text);
        return;
      }
      const file = aliases[pathname] ?? resolvePublic(pathname);
      if (file === null || !(await stat(file).catch(() => null))?.isFile()) {
        response.writeHead(404).end();
        return;
      }
      response.setHeader('content-type', CONTENT_TYPES[extname(file)] ?? 'application/octet-stream');
      const text = await readFile(file, 'utf8');
      response.end(extname(file) === '.ts' ? stripTypeScriptTypes(text) : text);
    };
    send().catch((error: unknown) => {
      console.error(error);
      if (!response.headersSent) response.writeHead(500, { 'content-type': 'text/plain; charset=utf-8' });
      response.end(error instanceof Error ? error.message : '');
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
            engine.close();
            server.close(() => {
              done();
            });
            server.closeAllConnections();
          }),
      });
    });
  });
}
