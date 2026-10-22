import { readJson, readText, writeJson } from '../files.ts';
import { describePlaceIssue, parsePlaces } from '../places.ts';
import { runPaths, runProfile } from '../run.ts';
import { solveTour } from '../solve/solve.ts';
import { loadRun, usage } from './shared.ts';

/** Un OSRM par profil : chacun a ses propres données préparées (voir docker-compose.yml). */
const OSRM_URLS = {
  car: process.env['OSRM_URL'] ?? 'http://localhost:5000',
  foot: process.env['OSRM_FOOT_URL'] ?? 'http://localhost:5001',
  train: process.env['OSRM_TRAIN_URL'] ?? 'http://localhost:5002',
} as const;

/**
 * `node src/cli/solve.ts <run.json>` → `data/<run>/tour.json`.
 * Seul lecteur de `OSRM_URL`, `OSRM_FOOT_URL`, `OSRM_TRAIN_URL` et `VROOM_URL`.
 */
export async function main(argv: readonly string[]): Promise<number> {
  const [runFile] = argv;
  if (runFile === undefined) return usage('solve <run.json>');
  const run = await loadRun(runFile);
  const profile = runProfile(await readJson(runFile));
  if (profile === null) console.error(`${runFile} : profile attendu car | foot | train`);
  if (run === null || profile === null) return 1;
  const paths = runPaths(run);
  const places = parsePlaces(await readText(paths.places), run.bbox);
  if (!places.ok) {
    for (const issue of places.errors) console.error(describePlaceIssue(issue));
    return 1;
  }
  const tour = await solveTour(run, places.value, {
    osrmUrl: OSRM_URLS[profile],
    vroomUrl: process.env['VROOM_URL'] ?? 'http://localhost:3000',
  });
  for (const place of tour.excluded) console.warn(`Inaccessible par la route, exclu : ${place.id} (${place.name})`);
  await writeJson(paths.tour, tour);
  const km = (tour.path.meters[tour.path.meters.length - 1] ?? 0) / 1000;
  console.log(`${paths.tour} : ${String(tour.stops.length)} lieux, ${km.toFixed(1)} km.`);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
