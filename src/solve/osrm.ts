import type { LonLat, NonEmpty } from '../geo.ts';
import { isFiniteNumber, isLonLatArray, isRecord } from '../narrow.ts';

/** Distances en mètres, `null` quand OSRM ne trouve aucune route. */
export type DistanceMatrix = readonly (readonly (number | null)[])[];

export type DistanceTable = {
  readonly matrix: DistanceMatrix;
  /** Distance entre chaque lieu et le point du réseau où OSRM l'a projeté, en mètres. */
  readonly snapMeters: readonly number[];
};

export type RoutedPath = {
  readonly points: NonEmpty<LonLat>;
  /** Nombre de segments de `points` couverts par chaque étape, dans l'ordre des waypoints. */
  readonly legSegments: readonly number[];
  /** Traversées en bateau (bac, pirogue…) : indices `[début, fin]` dans `points`, dans l'ordre. */
  readonly ferrySegments: readonly (readonly [number, number])[];
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
export async function fetchDistanceMatrix(osrmUrl: string, locations: readonly LonLat[]): Promise<DistanceTable> {
  // ponytail: toutes les coordonnées dans l'URL ; découper en blocs sources/destinations si osrm-routed refuse.
  const raw = await getOsrm(`${osrmUrl}/table/v1/driving/${coordinatesPath(locations)}?annotations=distance`);
  const distances = raw['distances'];
  if (!Array.isArray(distances) || distances.length !== locations.length) {
    throw new OsrmError('matrice de distances absente ou de mauvaise taille');
  }
  const rows: readonly unknown[] = distances;
  const matrix = rows.map((row) => {
    if (!Array.isArray(row) || row.length !== locations.length) throw new OsrmError('ligne de matrice invalide');
    const cells: readonly unknown[] = row;
    return cells.map((cell) => (isFiniteNumber(cell) ? cell : null));
  });
  const sources: readonly unknown[] = Array.isArray(raw['sources']) ? raw['sources'] : [];
  const snapMeters = locations.map((_, i) => {
    const snapped = sources[i];
    return isRecord(snapped) && isFiniteNumber(snapped['distance']) ? snapped['distance'] : 0;
  });
  return { matrix, snapMeters };
}

export async function fetchRoute(osrmUrl: string, waypoints: readonly LonLat[]): Promise<RoutedPath> {
  // `continue_straight=false` : demi-tour permis à chaque lieu, comme dans la matrice ; sinon un lieu au
  // fond d'une impasse ou sur un sens unique rend la tournée entière introuvable (NoRoute).
  const raw = await getOsrm(
    `${osrmUrl}/route/v1/driving/${coordinatesPath(waypoints)}?overview=full&geometries=geojson&annotations=distance&steps=true&continue_straight=false`,
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
  return {
    points: [first, ...rest],
    legSegments,
    ferrySegments: ferrySegments(rawLegs, segments),
    meters: route['distance'],
  };
}

/**
 * Traversées en bateau d'après les `steps` (mode `ferry`) : chaque step couvre autant de segments que sa
 * géométrie, qui reprend le dernier point du step précédent, sauf l'arrivée d'une étape (le point
 * d'arrivée deux fois, aucun segment). Sans steps (vieux OSRM, tests) : aucune.
 */
export function ferrySegments(legs: readonly unknown[], segments: number): (readonly [number, number])[] {
  const steps = legs.flatMap((leg) => {
    const raw: unknown = isRecord(leg) ? leg['steps'] : undefined;
    return Array.isArray(raw) ? raw.filter(isRecord) : [];
  });
  if (steps.length === 0) return [];
  const ferries: [number, number][] = [];
  let vertex = 0;
  for (const step of steps) {
    const geometry = step['geometry'];
    const coordinates = isRecord(geometry) ? geometry['coordinates'] : undefined;
    if (!Array.isArray(coordinates)) throw new OsrmError('step sans géométrie');
    const maneuver = step['maneuver'];
    const arrival = isRecord(maneuver) && maneuver['type'] === 'arrive';
    const end = arrival ? vertex : vertex + Math.max(0, coordinates.length - 1);
    const previous = ferries.at(-1);
    if (step['mode'] === 'ferry' && end > vertex) {
      // Deux steps de bateau qui se suivent (escale) : une seule traversée.
      if (previous !== undefined && previous[1] === vertex) previous[1] = end;
      else ferries.push([vertex, end]);
    }
    vertex = end;
  }
  if (vertex !== segments)
    throw new OsrmError(`steps incohérents : ${String(vertex)} segments pour ${String(segments)}`);
  return ferries;
}
