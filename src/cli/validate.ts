import { readText } from '../files.ts';
import { describePlaceIssue, parsePlaces } from '../places.ts';
import { runPaths } from '../run.ts';
import { loadRun, usage } from './shared.ts';

/** `node src/cli/validate.ts <run.json>` : valide `runs/<run>/places.geojson`. Code 0 si conforme. */
export async function main(argv: readonly string[]): Promise<number> {
  const [runFile] = argv;
  if (runFile === undefined) return usage('validate <run.json>');
  const run = await loadRun(runFile);
  if (run === null) return 1;
  const file = runPaths(run).places;
  const result = parsePlaces(await readText(file), run.bbox);
  if (!result.ok) {
    for (const issue of result.errors) console.error(describePlaceIssue(issue));
    return 1;
  }
  console.log(`${file} : ${String(result.value.length)} lieux conformes.`);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
