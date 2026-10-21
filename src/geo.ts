export type LonLat = readonly [lon: number, lat: number];

export type NonEmpty<T> = readonly [T, ...T[]];

export type BBox = {
  readonly west: number;
  readonly south: number;
  readonly east: number;
  readonly north: number;
};

/** `meters[i]` est la distance parcourue en arrivant à `points[i]`. */
export type MeasuredPath = {
  readonly points: NonEmpty<LonLat>;
  readonly meters: NonEmpty<number>;
};

const EARTH_RADIUS_METERS = 6_371_008.8;

const toRadians = (degrees: number): number => (degrees * Math.PI) / 180;

export function distanceMeters(a: LonLat, b: LonLat): number {
  const dLat = toRadians(b[1] - a[1]);
  const dLon = toRadians(b[0] - a[0]);
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(toRadians(a[1])) * Math.cos(toRadians(b[1])) * Math.sin(dLon / 2) ** 2;
  return 2 * EARTH_RADIUS_METERS * Math.asin(Math.min(1, Math.sqrt(h)));
}

export function inBBox(point: LonLat, bbox: BBox): boolean {
  const [lon, lat] = point;
  return lon >= bbox.west && lon <= bbox.east && lat >= bbox.south && lat <= bbox.north;
}

export function measurePath(points: NonEmpty<LonLat>): MeasuredPath {
  let total = 0;
  const [first, ...rest] = points;
  const meters: [number, ...number[]] = [0];
  let previous = first;
  for (const point of rest) {
    total += distanceMeters(previous, point);
    meters.push(total);
    previous = point;
  }
  return { points, meters };
}

export function totalMeters(path: MeasuredPath): number {
  return path.meters[path.meters.length - 1] ?? 0;
}

/** Indice `i` du segment [i, i + 1] qui contient `meters`, supposé dans ]0, longueur[. */
function segmentIndex(path: MeasuredPath, meters: number): number {
  let low = 0;
  let high = path.meters.length - 1;
  while (high - low > 1) {
    const middle = (low + high) >> 1;
    if ((path.meters[middle] ?? 0) < meters) low = middle;
    else high = middle;
  }
  return low;
}

function interpolate(path: MeasuredPath, index: number, meters: number): LonLat {
  const a = path.points[index] ?? path.points[0];
  const b = path.points[index + 1] ?? a;
  const start = path.meters[index] ?? 0;
  const length = (path.meters[index + 1] ?? start) - start;
  const ratio = length > 0 ? (meters - start) / length : 0;
  return [a[0] + (b[0] - a[0]) * ratio, a[1] + (b[1] - a[1]) * ratio];
}

/** Ramené aux extrémités hors de [0, longueur]. */
export function pointAt(path: MeasuredPath, meters: number): LonLat {
  const last = path.points[path.points.length - 1] ?? path.points[0];
  if (meters <= 0) return path.points[0];
  if (meters >= totalMeters(path)) return last;
  return interpolate(path, segmentIndex(path, meters), meters);
}

/** Tracé parcouru jusqu'à `meters`, extrémité interpolée ; vide à 0 m. */
export function pathUntil(path: MeasuredPath, meters: number): LonLat[] {
  if (meters <= 0) return [];
  if (meters >= totalMeters(path)) return [...path.points];
  const index = segmentIndex(path, meters);
  return [...path.points.slice(0, index + 1), interpolate(path, index, meters)];
}

/**
 * Sous-ensemble des sommets espacés d'au moins `minMeters`, distances cumulées d'origine conservées :
 * `pointAt` et `pathUntil` restent alignés sur le tracé complet.
 */
export function decimate(path: MeasuredPath, minMeters: number): MeasuredPath {
  const points: [LonLat, ...LonLat[]] = [path.points[0]];
  const meters: [number, ...number[]] = [path.meters[0]];
  let lastKept = path.meters[0];
  const lastIndex = path.points.length - 1;
  path.points.forEach((point, i) => {
    const at = path.meters[i] ?? lastKept;
    if (i === 0 || (i !== lastIndex && at - lastKept < minMeters)) return;
    points.push(point);
    meters.push(at);
    lastKept = at;
  });
  return { points, meters };
}
