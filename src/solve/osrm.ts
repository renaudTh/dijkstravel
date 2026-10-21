import type { LonLat, NonEmpty } from '../geo.ts';
import { isFiniteNumber, isLonLatArray, isRecord } from '../narrow.ts';

/** Distances en mètres, `null` quand OSRM ne trouve aucune route. */
export type DistanceMatrix = readonly (readonly (number | null)[])[];

export type RoutedPath = {
  readonly points: NonEmpty<LonLat>;
  /** Nombre de segments de `points` couverts par chaque étape, dans l'ordre des waypoints. */
  readonly legSegments: readonly number[];
  readonly meters: number;
};

export class OsrmError extends Error {
  readonly reason: string;

  constructor(reason: string) {
    super(`OSRM : ${reason}`);
    this.reason = reason;
  }
}

const coordinatesPath = (points: readonly LonLat[]): string =>
  points.map(([lon, lat]) => `${lon.toFixed(6)},${lat.toFixed(6)}`).join(';');

async function getOsrm(url: string): Promise<Readonly<Record<string, unknown>>> {
  const response = await fetch(url);
  const raw: unknown = await response.json().catch(() => null);
  if (!isRecord(raw)) throw new OsrmError(`HTTP ${String(response.status)} sans corps JSON`);
  if (raw['code'] !== 'Ok') throw new OsrmError(`${String(raw['code'])} : ${String(raw['message'])}`);
  return raw;
}

// Le profil de l'URL (`driving`) est ignoré par osrm-routed : il sert le profil de ses données (car, foot…).
export async function fetchDistanceMatrix(osrmUrl: string, locations: readonly LonLat[]): Promise<DistanceMatrix> {
  // ponytail: toutes les coordonnées dans l'URL ; découper en blocs sources/destinations si osrm-routed refuse.
  const raw = await getOsrm(`${osrmUrl}/table/v1/driving/${coordinatesPath(locations)}?annotations=distance`);
  const distances = raw['distances'];
  if (!Array.isArray(distances) || distances.length !== locations.length) {
    throw new OsrmError('matrice de distances absente ou de mauvaise taille');
  }
  const rows: readonly unknown[] = distances;
  return rows.map((row) => {
    if (!Array.isArray(row) || row.length !== locations.length) throw new OsrmError('ligne de matrice invalide');
    const cells: readonly unknown[] = row;
    return cells.map((cell) => (isFiniteNumber(cell) ? cell : null));
  });
}

export async function fetchRoute(osrmUrl: string, waypoints: readonly LonLat[]): Promise<RoutedPath> {
  const raw = await getOsrm(
    `${osrmUrl}/route/v1/driving/${coordinatesPath(waypoints)}?overview=full&geometries=geojson&annotations=distance`,
  );
  const routes = raw['routes'];
  const route: unknown = Array.isArray(routes) ? routes[0] : undefined;
  if (!isRecord(route) || !isFiniteNumber(route['distance']) || !isRecord(route['geometry'])) {
    throw new OsrmError('route absente');
  }
  const coordinates = route['geometry']['coordinates'];
  const legs = route['legs'];
  if (!Array.isArray(coordinates) || !Array.isArray(legs)) throw new OsrmError('géométrie ou étapes absentes');
  const rawPoints: readonly unknown[] = coordinates;
  const rawLegs: readonly unknown[] = legs;
  const points = rawPoints.filter(isLonLatArray);
  const legSegments = rawLegs.map((leg) => {
    const annotation = isRecord(leg) ? leg['annotation'] : undefined;
    const distances = isRecord(annotation) ? annotation['distance'] : undefined;
    if (!Array.isArray(distances)) throw new OsrmError('étape sans annotation de distance');
    return distances.length;
  });
  const [first, ...rest] = points;
  const segments = legSegments.reduce((sum, count) => sum + count, 0);
  if (first === undefined || points.length !== rawPoints.length || segments !== points.length - 1) {
    throw new OsrmError(`découpage incohérent : ${String(segments)} segments pour ${String(points.length)} points`);
  }
  return { points: [first, ...rest], legSegments, meters: route['distance'] };
}
