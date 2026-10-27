import { serveEditor } from '../render/server.ts';
import { loadRun, loadThemeFile, usage } from './shared.ts';

/** `node src/cli/editor.ts <run.json>` : éditeur du thème du run, sur sa tournée déjà calculée. */
export async function main(argv: readonly string[]): Promise<number> {
  const [runFile] = argv;
  if (runFile === undefined) return usage('editor <run.json>');
  const run = await loadRun(runFile);
  const theme = await loadThemeFile(runFile);
  if (run === null || theme === null) return 1;
  const server = await serveEditor({ runFile, run, theme });
  console.log(`Éditeur : ${server.url}editor  (thème ${theme}, Ctrl+C pour arrêter)`);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
