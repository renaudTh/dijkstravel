import { access, writeFile } from 'node:fs/promises';
import { readJson } from '../files.ts';
import { frameCount, timeline } from '../render/frame.ts';
import { prefetchTiles, renderFrame, writeInput } from '../render/native.ts';
import { runPaths, runPublication } from '../run.ts';
import { parseTour } from '../solve/tour.ts';
import { loadRun, loadThemeFile, usage } from './shared.ts';

const km = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 0 });

/**
 * `node src/cli/publication.ts <run.json>` → `data/out/<run>/publication.txt` (légende prête à coller) et,
 * `data/out/<run>/cover.jpg` (dernière image, tracé complet dézoomé, sans la carte d'infos : place libre pour un titre).
 */
export async function main(argv: readonly string[]): Promise<number> {
  const [runFile] = argv;
  if (runFile === undefined) return usage('publication <run.json>');
  const run = await loadRun(runFile);
  const theme = await loadThemeFile(runFile);
  if (run === null || theme === null) return 1;
  const raw = await readJson(runFile);
  const publication = runPublication(raw);
  if (publication === null) {
    console.error(`${runFile} : aucune section publication (caption, hashtags).`);
    return 1;
  }
  const paths = runPaths(run);
  const tour = parseTour(await readJson(paths.tour));
  const total = tour.path.meters[tour.path.meters.length - 1] ?? 0;
  const caption = publication.caption
    .replaceAll('{lieux}', String(tour.stops.length))
    .replaceAll('{km}', `${km.format(total / 1000)} km`);

  const hasVideo = await access(paths.video).then(
    () => true,
    () => false,
  );
  const cover = `${paths.out}/cover.jpg`;
  // Toute dernière image : fin de l'outro, tracé complet dézoomé, sans la carte d'infos.
  const input = await writeInput(runFile, run, theme);
  const last = frameCount(timeline(run.video)) - 1;
  await prefetchTiles(input, `--frame=${String(last)}`);
  await renderFrame(input, last, cover, false);

  const text = [
    '=== LÉGENDE (à coller telle quelle) ===',
    '',
    caption,
    '',
    publication.hashtags.join(' '),
    '',
    '=== À PRÉPARER ===',
    '',
    hasVideo ? `Vidéo : ${paths.video}` : `Vidéo : pas encore rendue → npm run render -- ${runFile}`,
    `Couverture : ${cover} (tracé complet dézoomé, sans la carte de titre, à choisir comme miniature)`,
    "Heure : en semaine vers 18 h - 21 h, ou le week-end vers midi (repère général : à ajuster d'après vos statistiques).",
    'Sources : la mention OpenStreetMap / imagerie est déjà dans la vidéo, rien à ajouter dans la légende.',
    '',
  ].join('\n');
  await writeFile(`${paths.out}/publication.txt`, text);
  console.log(text);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
