import { basename, dirname, join } from 'node:path';
import { readJson, writeJson } from '../files.ts';
import { decimate, measurePath, pointAt, splitCrossings, totalMeters, type LonLat } from '../geo.ts';
import type { VideoSettings } from '../run.ts';
import { parseTour, type Tour } from '../solve/tour.ts';
import { cameraAt, frameCount, hudAt, momentAt, timeline } from './frame.ts';

/**
 * Valeurs de référence pour le moteur Rust (`render/tests/parity.rs`) : caméra, HUD et géométrie
 * calculées par le TS sur des tournées synthétiques. `node src/render/parity-fixtures.ts [tour.json…]`
 * ajoute de vraies tournées, écrites à part (`<dossier>/real-<run>.json`, hors dépôt).
 */
const OUT_DIR = join(import.meta.dirname, '..', '..', 'render', 'tests', 'fixtures');

type Case = {
  readonly name: string;
  readonly tour: Tour;
  readonly video: VideoSettings;
  readonly followFraction: number;
  readonly slowdown: number;
};

/** Générateur pseudo-aléatoire déterministe (mulberry32) : les tournées ne changent pas d'un appel à l'autre. */
function random(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function syntheticTour(name: string, seed: number, points: number, stops: number, ferries: number): Tour {
  const next = random(seed);
  const walk: [LonLat, ...LonLat[]] = [[-1.5 + next(), 46 + next()]];
  for (let i = 1; i < points; i++) {
    const [lon, lat] = walk[i - 1] ?? walk[0];
    // Pas variés, dont des points répétés : segments de longueur nulle compris.
    const step = next() < 0.02 ? 0 : 0.004 + next() * 0.02;
    const heading = (i / points) * 2 * Math.PI + (next() - 0.5);
    walk.push([lon + step * Math.cos(heading), lat + step * Math.sin(heading)]);
  }
  const path = measurePath(walk);
  const total = totalMeters(path);
  const stopMeters = Array.from({ length: stops }, () => next() * total).sort((a, b) => a - b);
  const crossings = Array.from({ length: ferries }, (_, i): [number, number] => {
    const start = ((i + 0.3) / ferries) * total;
    return [start, start + total * 0.02];
  });
  return {
    run: name,
    mode: 'open',
    start: walk[0],
    path,
    stops: stopMeters.map((meters, i) => ({
      id: `s${String(i)}`,
      name: `Lieu ${String(i)} · Département`,
      at: pointAt(path, meters),
      meters,
    })),
    excluded: [],
    ferries: crossings,
    routedMeters: total,
  };
}

/** Une polyligne résumée (nombre, extrémités, somme des coordonnées) : assez pour la comparer, sans alourdir le dépôt. */
type Summary = {
  readonly count: number;
  readonly first: LonLat | null;
  readonly last: LonLat | null;
  readonly sum: readonly [number, number];
};

function summary(points: readonly LonLat[]): Summary {
  const sum = points.reduce<[number, number]>((acc, [lon, lat]) => [acc[0] + lon, acc[1] + lat], [0, 0]);
  return { count: points.length, first: points[0] ?? null, last: points.at(-1) ?? null, sum };
}

function fixture(test: Case): unknown {
  const { tour, video, followFraction, slowdown } = test;
  const frames = timeline(video);
  const count = frameCount(frames);
  const total = totalMeters(tour.path);
  // Bords de phase et un échantillon régulier : quelques centaines d'images suffisent.
  const sampled = new Set([0, 1, frames.intro - 1, frames.intro, frames.intro + frames.travel - 1, count - 1]);
  for (let frame = 0; frame < count; frame += Math.max(1, Math.floor(count / 300))) sampled.add(frame);
  const probes = Array.from({ length: 41 }, (_, i) => (total * (i - 2)) / 36);
  return {
    name: test.name,
    video: { ...video, width: 1080, height: 1920 },
    followFraction,
    slowdown,
    tour: { path: tour.path, stops: tour.stops, ferries: tour.ferries },
    frameCount: count,
    frames: [...sampled]
      .filter((frame) => frame >= 0 && frame < count)
      .sort((a, b) => a - b)
      .map((frame) => {
        const moment = momentAt(frames, tour, frame, slowdown);
        const meters = moment.phase === 'travel' ? moment.meters : moment.phase === 'intro' ? 0 : total;
        return { frame, moment, camera: cameraAt(tour, moment, followFraction), hud: hudAt(tour, meters) };
      }),
    pointAt: probes.map((meters) => ({ meters, point: pointAt(tour.path, meters) })),
    crossings: probes.map((meters) => ({
      meters,
      pieces: splitCrossings(tour.path, tour.ferries, meters).map((piece) => ({
        ferry: piece.ferry,
        ...summary(piece.points),
      })),
    })),
    decimate: [total / 50, total / 2000].map((minMeters) => {
      const { points, meters } = decimate(tour.path, minMeters);
      return { minMeters, points: summary(points), meters: meters.reduce((a, b) => a + b, 0) };
    }),
  };
}

const VIDEO: VideoSettings = { format: '9:16', durationSec: 20, introSec: 2, outroSec: 3, fps: 30 };

const CASES: readonly Case[] = [
  {
    name: 'slowdown-ferries',
    tour: syntheticTour('slowdown-ferries', 1, 3000, 30, 2),
    video: VIDEO,
    followFraction: 1 / 8,
    slowdown: 1.2,
  },
  {
    name: 'constant-pace',
    tour: syntheticTour('constant-pace', 2, 800, 5, 0),
    video: { ...VIDEO, format: '16:9', introSec: 0, outroSec: 1, fps: 25 },
    followFraction: 0.3,
    slowdown: 0,
  },
  {
    name: 'tiny',
    tour: syntheticTour('tiny', 3, 2, 0, 0),
    video: { ...VIDEO, durationSec: 1 / 30, introSec: 0, outroSec: 0 },
    followFraction: 1,
    slowdown: 3,
  },
];

for (const test of CASES) await writeJson(join(OUT_DIR, `${test.name}.json`), fixture(test));
for (const file of process.argv.slice(2)) {
  const tour = parseTour(await readJson(file));
  const name = basename(dirname(file));
  const out = join(OUT_DIR, 'real', `${name}.json`);
  await writeJson(
    out,
    fixture({ name, tour, video: { ...VIDEO, durationSec: 90 }, followFraction: 1 / 8, slowdown: 1.2 }),
  );
  console.log(out);
}
