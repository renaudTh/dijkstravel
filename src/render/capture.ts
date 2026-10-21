import { spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import { rm, writeFile } from 'node:fs/promises';
import { basename } from 'node:path';
import { chromium, type BrowserContext } from 'playwright';

export class FfmpegError extends Error {
  readonly code: number | null;

  constructor(code: number | null) {
    super(`ffmpeg a échoué (code ${String(code)})`);
    this.code = code;
  }
}

/** Cache HTTP de Chromium conservé d'un rendu à l'autre : les tuiles ne sont téléchargées qu'une fois. */
const BROWSER_PROFILE = 'data/.browser-profile';

function runFfmpeg(
  args: readonly string[],
  input: 'pipe' | 'ignore',
): { readonly ffmpeg: ChildProcess; readonly exited: Promise<void> } {
  const ffmpeg = spawn('ffmpeg', ['-y', '-loglevel', 'error', ...args], { stdio: [input, 'inherit', 'inherit'] });
  const exited = new Promise<void>((resolve, reject) => {
    ffmpeg.on('close', (code) => {
      if (code === 0) resolve();
      else reject(new FfmpegError(code));
    });
  });
  return { ffmpeg, exited };
}

/** Découpe [0, frames) en `count` tranches contiguës, la dernière éventuellement plus courte. */
export function chunks(frames: number, count: number): { readonly start: number; readonly end: number }[] {
  const size = Math.ceil(frames / Math.max(1, count));
  return Array.from({ length: Math.ceil(frames / size) }, (_, i) => ({
    start: i * size,
    end: Math.min(frames, (i + 1) * size),
  }));
}

async function renderSegment(
  context: BrowserContext,
  options: { readonly pageUrl: string; readonly fps: number; readonly start: number; readonly end: number },
  segmentFile: string,
  onFrame: () => void,
): Promise<void> {
  const { ffmpeg, exited } = runFfmpeg(
    [
      ['-f', 'image2pipe', '-c:v', 'mjpeg', '-framerate', String(options.fps), '-i', '-'],
      ['-c:v', 'libx264', '-preset', 'veryfast', '-crf', '23', '-pix_fmt', 'yuv420p'],
      [segmentFile],
    ].flat(),
    'pipe',
  );
  const stdin = ffmpeg.stdin;
  if (stdin === null) throw new FfmpegError(null);
  const page = await context.newPage();
  page.on('pageerror', (error) => {
    console.error(error);
  });
  try {
    await page.goto(options.pageUrl);
    await page.waitForFunction(() => window.frameCount > 0, undefined, { timeout: 120_000 });
    // Séquentiel dans une tranche : la page et ses tuiles en mémoire servent à l'image suivante.
    for (let frame = options.start; frame < options.end; frame++) {
      await page.evaluate((index) => window.renderFrame(index), frame);
      const image = await page.screenshot({ type: 'jpeg', quality: 92 });
      if (!stdin.write(image)) await once(stdin, 'drain');
      onFrame();
    }
  } finally {
    stdin.end();
    await page.close();
  }
  await exited;
}

/**
 * Rend la vidéo en `workers` tranches parallèles (une page Chromium et un ffmpeg chacune),
 * puis les recolle sans réencoder. Chaque image ne dépend que de son numéro.
 */
export async function captureVideo(options: {
  readonly pageUrl: string;
  readonly size: { readonly width: number; readonly height: number };
  readonly frames: number;
  readonly fps: number;
  readonly outFile: string;
  readonly workers: number;
}): Promise<void> {
  const { pageUrl, size, frames, fps, outFile, workers } = options;
  // GPU via ANGLE/OpenGL (environ 10× plus rapide que SwiftShader) ; SwiftShader reste le repli sans GPU.
  const context = await chromium.launchPersistentContext(BROWSER_PROFILE, {
    args: ['--use-angle=gl', '--ignore-gpu-blocklist', '--enable-unsafe-swiftshader'],
    viewport: size,
    deviceScaleFactor: 1,
  });
  const segments = chunks(frames, workers).map((range, i) => ({ ...range, file: `${outFile}.part${String(i)}.mp4` }));
  const list = `${outFile}.parts.txt`;
  let done = 0;
  const onFrame = (): void => {
    done++;
    if (done % fps === 0 || done === frames) process.stdout.write(`\rimage ${String(done)} / ${String(frames)}`);
  };
  try {
    await Promise.all(
      segments.map((segment) => renderSegment(context, { pageUrl, fps, ...segment }, segment.file, onFrame)),
    );
    process.stdout.write('\n');
    // Chemins relatifs au fichier de liste : c'est ainsi que le démultiplexeur concat les résout.
    await writeFile(list, segments.map((segment) => `file '${basename(segment.file)}'\n`).join(''));
    await runFfmpeg(
      ['-f', 'concat', '-safe', '0', '-i', list, '-c', 'copy', '-movflags', '+faststart', outFile],
      'ignore',
    ).exited;
  } finally {
    await context.close();
    await Promise.all([list, ...segments.map((segment) => segment.file)].map((file) => rm(file, { force: true })));
  }
}
