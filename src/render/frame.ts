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

/**
 * `slowdown` : temps passé de plus par mètre au passage d'un lieu (2 : trois fois plus lent), rattrapé
 * entre les lieux ; la durée du trajet ne change pas. 0 : allure constante.
 */
export function momentAt(timeline: Timeline, tour: Tour, frame: number, slowdown = 0): Moment {
  if (frame < timeline.intro) return { phase: 'intro', progress: progressOf(frame, timeline.intro) };
  const travelFrame = frame - timeline.intro;
  if (travelFrame < timeline.travel) {
    return { phase: 'travel', meters: metersAt(tour, progressOf(travelFrame, timeline.travel), slowdown) };
  }
  return { phase: 'outro', progress: progressOf(travelFrame - timeline.travel, timeline.outro) };
}

/** Distances échantillonnées le long du trajet et temps relatif cumulé pour atteindre chacune. */
type Pace = { readonly meters: readonly number[]; readonly time: readonly number[] };

const PACE_SAMPLES = 8192;
/** Demi-largeur de la zone ralentie autour d'un lieu, en part de l'écart moyen entre deux lieux. */
const SLOW_ZONE = 0.25;
const paces = new WeakMap<Tour, Map<number, Pace>>();

function metersAt(tour: Tour, progress: number, slowdown: number): number {
  const total = totalMeters(tour.path);
  if (slowdown === 0 || total === 0 || progress >= 1) return total * progress;
  const { meters, time } = paceOf(tour, slowdown);
  const target = progress * (time.at(-1) ?? 0);
  let [low, high] = [0, time.length - 1];
  while (high - low > 1) {
    const middle = (low + high) >> 1;
    if ((time[middle] ?? 0) <= target) low = middle;
    else high = middle;
  }
  const [t0, t1, m0, m1] = [time[low] ?? 0, time[high] ?? 0, meters[low] ?? 0, meters[high] ?? 0];
  return t1 === t0 ? m0 : lerp(m0, m1, (target - t0) / (t1 - t0));
}

function paceOf(tour: Tour, slowdown: number): Pace {
  const cached = paces.get(tour)?.get(slowdown);
  if (cached !== undefined) return cached;
  const total = totalMeters(tour.path);
  const radius = (SLOW_ZONE * total) / Math.max(1, tour.stops.length);
  const meters = Array.from({ length: PACE_SAMPLES + 1 }, (_, i) => (total * i) / PACE_SAMPLES);
  // ponytail: chaque échantillon contre chaque lieu, une fois par tournée (12 M d'opérations pour 1 500 lieux) ;
  // deux curseurs sur les lieux triés si une tournée en compte des dizaines de milliers.
  // Le plus fort ralentissement, pas leur somme : un groupe de lieux proches ne s'éternise pas.
  const slowness = meters.map(
    (at) => 1 + slowdown * Math.max(0, ...tour.stops.map((stop) => bump(Math.abs(at - stop.meters) / radius))),
  );
  const time = [0];
  for (let i = 1; i < meters.length; i++) {
    const step = (((slowness[i - 1] ?? 1) + (slowness[i] ?? 1)) / 2) * ((meters[i] ?? 0) - (meters[i - 1] ?? 0));
    time.push((time[i - 1] ?? 0) + step);
  }
  const pace = { meters, time };
  paces.set(tour, (paces.get(tour) ?? new Map<number, Pace>()).set(slowdown, pace));
  return pace;
}

/** 1 sur le lieu, 0 à partir d'une demi-zone, en cloche : la caméra freine et repart sans à-coup. */
const bump = (distance: number): number => (distance >= 1 ? 0 : (1 + Math.cos(Math.PI * distance)) / 2);

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

/** `followFraction` : part de l'emprise totale visible pendant le parcours (l'échelle d'une région pour la France). */
export function cameraAt(tour: Tour, moment: Moment, followFraction: number): Camera {
  switch (moment.phase) {
    case 'intro':
      return blend(overview(tour), travelCamera(tour, 0, followFraction), smoothstep(moment.progress));
    case 'travel':
      return travelCamera(tour, moment.meters, followFraction);
    case 'outro':
      return blend(
        travelCamera(tour, totalMeters(tour.path), followFraction),
        overview(tour),
        smoothstep(moment.progress),
      );
  }
}

/** Nord en haut, à plat ; le centre suit une moyenne glissante du tracé, pas la tête elle-même. */
function travelCamera(tour: Tour, meters: number, followFraction: number): Camera {
  const whole = overview(tour);
  const zoom = clamp(whole.zoom + Math.log2(1 / followFraction), 4, 16);
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
    zoom: clamp(Math.log2((SCREEN_PIXELS * 0.8 * 360) / (512 * span)), 1, 16),
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
