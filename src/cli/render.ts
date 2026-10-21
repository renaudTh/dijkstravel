import { readJson } from '../files.ts';
import { frameCount, timeline } from '../render/frame.ts';
import { captureVideo } from '../render/capture.ts';
import { servePage } from '../render/server.ts';
import { runPaths, VIDEO_SIZES } from '../run.ts';
import { parseTour } from '../solve/tour.ts';
import { loadRun, usage } from './shared.ts';

/**
 * `node src/cli/render.ts <run.json>` → `data/<run>/video.mp4`, à partir du seul `tour.json`.
 * `--preview` : sert la page et affiche son URL, sans capture.
 * `--seconds=<n>` : ne rend que les n premières secondes, pour itérer vite.
 * `--workers=<n>` : nombre de tranches rendues en parallèle (4 par défaut).
 */
export async function main(argv: readonly string[]): Promise<number> {
  const runFile = argv.find((arg) => !arg.startsWith('--'));
  if (runFile === undefined) return usage('render <run.json> [--preview] [--seconds=<n>] [--workers=<n>]');
  const secondsArg = argv.find((arg) => arg.startsWith('--seconds='));
  const seconds = secondsArg === undefined ? Infinity : Number(secondsArg.slice('--seconds='.length));
  const workersArg = argv.find((arg) => arg.startsWith('--workers='));
  const workers = workersArg === undefined ? 4 : Number(workersArg.slice('--workers='.length));
  if (!(seconds > 0) || !Number.isInteger(workers) || workers < 1) {
    return usage('render <run.json> [--preview] [--seconds=<n>] [--workers=<n>]');
  }
  const run = await loadRun(runFile);
  if (run === null) return 1;
  const paths = runPaths(run);
  // Lu ici pour échouer tôt ; la page relit le même fichier.
  parseTour(await readJson(paths.tour));
  const server = await servePage({ run: runFile, tour: paths.tour });
  if (argv.includes('--preview')) {
    console.log(`${server.url}?frame=0  (Ctrl+C pour arrêter)`);
    return 0;
  }
  try {
    await captureVideo({
      pageUrl: server.url,
      size: VIDEO_SIZES[run.video.format],
      frames: Math.min(frameCount(timeline(run.video)), Math.round(seconds * run.video.fps)),
      fps: run.video.fps,
      outFile: paths.video,
      workers,
    });
  } finally {
    await server.close();
  }
  console.log(paths.video);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
