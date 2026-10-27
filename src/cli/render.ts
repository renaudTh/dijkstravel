import { prefetchTiles, renderFrame, renderVideo, writeInput } from '../render/native.ts';
import { runPaths } from '../run.ts';
import { loadRun, loadThemeFile, usage } from './shared.ts';

const USAGE = 'render <run.json> [--seconds=<n>] [--threads=<n>] [--frame=<n>]';

/**
 * `node src/cli/render.ts <run.json>` → `data/<run>/video.mp4`, à partir du seul `tour.json`, par le moteur
 * natif (`render/`, sans GPU).
 * `--seconds=<n>` : ne rend que les n premières secondes, pour itérer vite.
 * `--threads=<n>` : threads de rendu (tous les cœurs par défaut).
 * `--frame=<n>` : l'image n seule, dans `data/<run>/frame-<n>.png`.
 */
export async function main(argv: readonly string[]): Promise<number> {
  const runFile = argv.find((arg) => !arg.startsWith('--'));
  if (runFile === undefined) return usage(USAGE);
  const option = (name: string): number | undefined => {
    const arg = argv.find((candidate) => candidate.startsWith(`--${name}=`));
    return arg === undefined ? undefined : Number(arg.slice(name.length + 3));
  };
  const [seconds, threads, frame] = [option('seconds'), option('threads'), option('frame')];
  const positiveInteger = (value: number | undefined) => value === undefined || (Number.isInteger(value) && value > 0);
  if (
    !(seconds === undefined || seconds > 0) ||
    !positiveInteger(threads) ||
    !(frame === undefined || (Number.isInteger(frame) && frame >= 0))
  ) {
    return usage(USAGE);
  }
  const run = await loadRun(runFile);
  const theme = await loadThemeFile(runFile);
  if (run === null || theme === null) return 1;
  const paths = runPaths(run);
  const input = await writeInput(runFile, run, theme);
  if (frame !== undefined) {
    await prefetchTiles(input, `--frame=${String(frame)}`);
    const outFile = `${paths.dir}/frame-${String(frame)}.png`;
    await renderFrame(input, frame, outFile);
    console.log(outFile);
    return 0;
  }
  const frames = seconds === undefined ? undefined : Math.round(seconds * run.video.fps);
  await prefetchTiles(input, frames === undefined ? undefined : `--frames=${String(frames)}`);
  await renderVideo(input, paths.video, { frames, threads });
  console.log(paths.video);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
