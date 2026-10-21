import { readText, writeJson } from '../files.ts';
import { describePlaceIssue, parsePlaces } from '../places.ts';
import { runPaths } from '../run.ts';
import { solveTour } from '../solve/solve.ts';
import { loadRun, usage } from './shared.ts';

/** `node src/cli/solve.ts <run.json>` → `data/<run>/tour.json`. Seul lecteur de `OSRM_URL` et `VROOM_URL`. */
export async function main(argv: readonly string[]): Promise<number> {
  const [runFile] = argv;
  if (runFile === undefined) return usage('solve <run.json>');
  const run = await loadRun(runFile);
  if (run === null) return 1;
  const paths = runPaths(run);
  const places = parsePlaces(await readText(paths.places), run.bbox);
  if (!places.ok) {
    for (const issue of places.errors) console.error(describePlaceIssue(issue));
    return 1;
  }
  const tour = await solveTour(run, places.value, {
    osrmUrl: process.env['OSRM_URL'] ?? 'http://localhost:5000',
    vroomUrl: process.env['VROOM_URL'] ?? 'http://localhost:3000',
  });
  for (const place of tour.excluded) console.warn(`Inaccessible par la route, exclu : ${place.id} (${place.name})`);
  await writeJson(paths.tour, tour);
  const km = (tour.path.meters[tour.path.meters.length - 1] ?? 0) / 1000;
  console.log(`${paths.tour} : ${String(tour.stops.length)} lieux, ${km.toFixed(1)} km.`);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
