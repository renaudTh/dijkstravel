import { measurePath, totalMeters, type LonLat, type NonEmpty } from '../geo.ts';
import type { Place } from '../places.ts';
import type { Run, Start, TourMode } from '../run.ts';
import { fetchDistanceMatrix, fetchRoute, type DistanceMatrix, type RoutedPath } from './osrm.ts';
import type { ExcludedPlace, Stop, Tour } from './tour.ts';
import { buildVroomRequest, parseVroomSolution, postVroom, type Departure } from './vroom.ts';

export type Endpoints = { readonly osrmUrl: string; readonly vroomUrl: string };

/**
 * Au-delà, OSRM a projeté le lieu sur une route sans rapport (Camopi, sans route, accrochée à 86 km) :
 * le lieu est traité comme inaccessible. Large pour un grand stade, dont le centre est loin de la rue.
 */
export const MAX_SNAP_METERS = 2000;

export class UnknownStartPlaceError extends Error {
  readonly id: string;

  constructor(id: string) {
    super(`Lieu de départ absent de places.geojson : ${id}`);
    this.id = id;
  }
}

export class StartUnreachableError extends Error {
  readonly start: Start;

  constructor(start: Start) {
    super('Le point de départ est inaccessible par la route');
    this.start = start;
  }
}

export class RouteMismatchError extends Error {
  readonly pathMeters: number;
  readonly routedMeters: number;

  constructor(pathMeters: number, routedMeters: number) {
    super(`Tracé de ${String(pathMeters)} m pour ${String(routedMeters)} m annoncés par OSRM`);
    this.pathMeters = pathMeters;
    this.routedMeters = routedMeters;
  }
}

export class NoReachablePlaceError extends Error {
  constructor() {
    super('Aucun lieu accessible par la route');
  }
}

export async function solveTour(run: Run, places: NonEmpty<Place>, endpoints: Endpoints): Promise<Tour> {
  const { start, mode } = run;
  const startPlace = start.kind === 'place' ? places.find((place) => place.id === start.id) : undefined;
  if (start.kind === 'place' && startPlace === undefined) throw new UnknownStartPlaceError(start.id);
  const startAt = start.kind === 'coordinates' ? start.at : null;

  const locations = places.map((place) => place.at);
  if (startAt !== null) locations.push(startAt);
  const { matrix, snapMeters } = await fetchDistanceMatrix(endpoints.osrmUrl, locations);
  const snappedAway = snapMeters.flatMap((meters, index) => (meters > MAX_SNAP_METERS ? [index] : []));
  const unreachable = new Set([...findUnreachable(matrix), ...snappedAway]);
  const startIndex = startAt !== null ? places.length : startPlace === undefined ? -1 : places.indexOf(startPlace);
  if (unreachable.has(startIndex)) throw new StartUnreachableError(start);

  const kept = locations.flatMap((_, index) => (unreachable.has(index) ? [] : [index]));
  const reachable = places.filter((_, index) => !unreachable.has(index));
  const excluded = places.filter((_, index) => unreachable.has(index)).map(({ id, name }) => ({ id, name }));
  const subMatrix = kept.map((from) => kept.map((to) => matrix[from]?.[to] ?? null));

  const departure: Departure =
    startAt !== null
      ? { kind: 'index', index: reachable.length }
      : startPlace !== undefined
        ? { kind: 'index', index: reachable.indexOf(startPlace) }
        : mode === 'loop'
          ? { kind: 'index', index: 0 }
          : { kind: 'none' };
  const request = buildVroomRequest(subMatrix, reachable.length, departure, mode);
  const order = parseVroomSolution(await postVroom(endpoints.vroomUrl, request), reachable.length);
  const visits = order.flatMap((index) => {
    const place = reachable[index];
    return place === undefined ? [] : [place];
  });
  // Le lieu de départ est visité en premier, même si VROOM l'a placé ailleurs à coût nul.
  const origin = departure.kind === 'index' && startAt === null ? reachable[departure.index] : undefined;
  const [first, ...rest] = origin === undefined ? visits : [origin, ...visits.filter((place) => place !== origin)];
  if (first === undefined) throw new NoReachablePlaceError();
  const ordered: NonEmpty<Place> = [first, ...rest];

  const waypoints = routeWaypoints(startAt, ordered, mode);
  const route: RoutedPath =
    waypoints.length < 2
      ? { points: waypoints, legSegments: [], ferrySegments: [], meters: 0 }
      : await fetchRoute(endpoints.osrmUrl, waypoints);
  return assembleTour({ run, start: startAt, ordered, route, excluded });
}

/** Indices des lieux injoignables depuis la majorité des autres, ou qui ne la rejoignent pas. */
export function findUnreachable(matrix: DistanceMatrix): number[] {
  const others = matrix.length - 1;
  const isolated = (nulls: number): boolean => others > 0 && nulls * 2 > others;
  return matrix.flatMap((row, i) => {
    const rowNulls = row.filter((cell, j) => j !== i && cell === null).length;
    const columnNulls = matrix.filter((other, j) => j !== i && other[i] === null).length;
    return isolated(rowNulls) || isolated(columnNulls) ? [i] : [];
  });
}

/** Waypoints à router : départ, lieux dans l'ordre, retour au départ en boucle. */
export function routeWaypoints(start: LonLat | null, ordered: NonEmpty<Place>, mode: TourMode): NonEmpty<LonLat> {
  const origin = start ?? ordered[0].at;
  const visits = ordered.map((place) => place.at);
  const route = start === null ? visits : [start, ...visits];
  return mode === 'loop' ? [origin, ...route.slice(1), origin] : [origin, ...route.slice(1)];
}

/** Lève `RouteMismatchError` si la longueur du tracé s'écarte de plus de 1 % de celle d'OSRM. */
export function assembleTour(input: {
  readonly run: Run;
  readonly start: LonLat | null;
  readonly ordered: NonEmpty<Place>;
  readonly route: RoutedPath;
  readonly excluded: readonly ExcludedPlace[];
}): Tour {
  const { run, start, ordered, route, excluded } = input;
  const path = measurePath(route.points);
  const length = totalMeters(path);
  if (Math.abs(length - route.meters) > route.meters * 0.01) throw new RouteMismatchError(length, route.meters);

  // `arrivals[k]` : indice du point du tracé à la fin de l'étape k.
  const arrivals: number[] = [];
  route.legSegments.reduce((vertex, segments) => {
    arrivals.push(vertex + segments);
    return vertex + segments;
  }, 0);
  const stops: Stop[] = ordered.map((place, i) => {
    const vertex = start === null ? (i === 0 ? 0 : arrivals[i - 1]) : arrivals[i];
    return { id: place.id, name: place.name, at: place.at, meters: path.meters[vertex ?? 0] ?? length };
  });
  return {
    run: run.name,
    mode: run.mode,
    start: start ?? ordered[0].at,
    path,
    stops,
    excluded,
    ferries: route.ferrySegments.map(([from, to]) => [path.meters[from] ?? 0, path.meters[to] ?? length] as const),
    routedMeters: route.meters,
  };
}
