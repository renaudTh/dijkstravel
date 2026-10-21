// Chargé tel quel par le navigateur (types retirés par le serveur) : aucun import Node ici.
import { pointAt, totalMeters, type LonLat } from '../geo.ts';
import type { VideoSettings } from '../run.ts';
import type { Tour } from '../solve/tour.ts';

/** En nombre d'images. */
export type Timeline = { readonly intro: number; readonly travel: number; readonly outro: number };

export type Moment =
  | { readonly phase: 'intro'; readonly progress: number }
  | { readonly phase: 'travel'; readonly meters: number }
  | { readonly phase: 'outro'; readonly progress: number };

export type Hud = {
  readonly meters: number;
  readonly totalMeters: number;
  /** Les `visited` premiers lieux de `Tour.stops` sont visités. */
  readonly visited: number;
  readonly total: number;
  readonly lastStop: string | null;
};

export type Camera = {
  readonly center: LonLat;
  readonly zoom: number;
  readonly pitch: number;
  readonly bearing: number;
};

const SMOOTHING_SAMPLES = 21;
/** Pendant le parcours, l'écran couvre ce fragment de l'emprise totale : l'échelle d'une région pour la France. */
const FOLLOW_FRACTION = 1 / 8;
// ponytail: zoom calculé pour un écran de 1080 px de petit côté, vrai pour les deux formats de VIDEO_SIZES.
const SCREEN_PIXELS = 1080;
const WORLD_METERS = 40_075_016;

export function timeline(video: VideoSettings): Timeline {
  return {
    intro: Math.round(video.introSec * video.fps),
    travel: Math.max(1, Math.round(video.durationSec * video.fps)),
    outro: Math.round(video.outroSec * video.fps),
  };
}

export function frameCount(timeline: Timeline): number {
  return timeline.intro + timeline.travel + timeline.outro;
}

const progressOf = (index: number, count: number): number => (count <= 1 ? 1 : Math.min(1, index / (count - 1)));

export function momentAt(timeline: Timeline, tour: Tour, frame: number): Moment {
  if (frame < timeline.intro) return { phase: 'intro', progress: progressOf(frame, timeline.intro) };
  const travelFrame = frame - timeline.intro;
  if (travelFrame < timeline.travel) {
    return { phase: 'travel', meters: totalMeters(tour.path) * progressOf(travelFrame, timeline.travel) };
  }
  return { phase: 'outro', progress: progressOf(travelFrame - timeline.travel, timeline.outro) };
}

/** Un lieu est visité dès que `meters` atteint sa distance d'arrivée. */
export function hudAt(tour: Tour, meters: number): Hud {
  const total = totalMeters(tour.path);
  const clamped = Math.min(total, Math.max(0, meters));
  const visited = tour.stops.filter((stop) => stop.meters <= clamped).length;
  return {
    meters: clamped,
    totalMeters: total,
    visited,
    total: tour.stops.length,
    lastStop: tour.stops[visited - 1]?.name ?? null,
  };
}

export function cameraAt(tour: Tour, moment: Moment): Camera {
  switch (moment.phase) {
    case 'intro':
      return blend(overview(tour), travelCamera(tour, 0), smoothstep(moment.progress));
    case 'travel':
      return travelCamera(tour, moment.meters);
    case 'outro':
      return blend(travelCamera(tour, totalMeters(tour.path)), overview(tour), smoothstep(moment.progress));
  }
}

/** Nord en haut, à plat ; le centre suit une moyenne glissante du tracé, pas la tête elle-même. */
function travelCamera(tour: Tour, meters: number): Camera {
  const whole = overview(tour);
  const zoom = clamp(whole.zoom + Math.log2(1 / FOLLOW_FRACTION), 4, 13);
  const visibleMeters = (WORLD_METERS * Math.cos(toRadians(whole.center[1])) * SCREEN_PIXELS) / (512 * 2 ** zoom);
  const window = visibleMeters * 0.5;
  const samples = Array.from({ length: SMOOTHING_SAMPLES }, (_, i) =>
    pointAt(tour.path, meters + window * ((2 * i) / (SMOOTHING_SAMPLES - 1) - 1)),
  );
  return { center: average(samples), zoom, pitch: 0, bearing: 0 };
}

/** Une fois par tournée : parcourir 700 000 points à chaque image coûterait pour rien. */
const overviews = new WeakMap<Tour, Camera>();

function overview(tour: Tour): Camera {
  const cached = overviews.get(tour);
  if (cached !== undefined) return cached;
  // Pas de `Math.min(...points)` : au-delà de ~100 000 points, l'étalement dépasse la pile.
  let [west, east, south, north] = [Infinity, -Infinity, Infinity, -Infinity];
  for (const [lon, lat] of tour.path.points) {
    const y = mercatorDegrees(lat);
    [west, east, south, north] = [Math.min(west, lon), Math.max(east, lon), Math.min(south, y), Math.max(north, y)];
  }
  const span = Math.max(east - west, north - south, 0.01);
  const camera: Camera = {
    center: [(west + east) / 2, inverseMercatorDegrees((south + north) / 2)],
    zoom: clamp(Math.log2((SCREEN_PIXELS * 0.8 * 360) / (512 * span)), 1, 13),
    pitch: 0,
    bearing: 0,
  };
  overviews.set(tour, camera);
  return camera;
}

function blend(from: Camera, to: Camera, t: number): Camera {
  return {
    center: [lerp(from.center[0], to.center[0], t), lerp(from.center[1], to.center[1], t)],
    zoom: lerp(from.zoom, to.zoom, t),
    pitch: 0,
    bearing: 0,
  };
}

function average(points: readonly LonLat[]): LonLat {
  const sum = points.reduce<[number, number]>((acc, point) => [acc[0] + point[0], acc[1] + point[1]], [0, 0]);
  return [sum[0] / points.length, sum[1] / points.length];
}

const lerp = (a: number, b: number, t: number): number => a + (b - a) * t;
const clamp = (value: number, min: number, max: number): number => Math.min(max, Math.max(min, value));
const smoothstep = (t: number): number => t * t * (3 - 2 * t);
const toRadians = (degrees: number): number => (degrees * Math.PI) / 180;
const mercatorDegrees = (lat: number): number => (Math.log(Math.tan(Math.PI / 4 + toRadians(lat) / 2)) * 180) / Math.PI;
const inverseMercatorDegrees = (y: number): number =>
  (2 * Math.atan(Math.exp((y * Math.PI) / 180)) - Math.PI / 2) * (180 / Math.PI);
