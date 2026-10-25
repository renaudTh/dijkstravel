import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { isDeepStrictEqual } from 'node:util';
import type { LonLat, NonEmpty } from '../geo.ts';
import type { Place } from '../places.ts';
import { DEFAULT_VIDEO } from '../run.ts';
import type { Run, Start, TourMode } from '../run.ts';
import type { RoutedPath } from './osrm.ts';
import {
  assembleTour,
  findUnreachable,
  RouteMismatchError,
  routeWaypoints,
  solveTour,
  StartUnreachableError,
  UnknownStartPlaceError,
} from './solve.ts';

const OSRM_URL = 'http://osrm.test';
const VROOM_URL = 'http://vroom.test';
const ENDPOINTS = { osrmUrl: OSRM_URL, vroomUrl: VROOM_URL };

const RENNES: Place = { id: 'a', name: 'Rennes', at: [-1.68, 48.11], source: null };
const BREST: Place = { id: 'b', name: 'Brest', at: [-4.49, 48.39], source: null };
const VANNES: Place = { id: 'c', name: 'Vannes', at: [-2.76, 47.66], source: null };
const SEIN: Place = { id: 'sein', name: 'Île de Sein', at: [-4.85, 48.04], source: null };
const PLOERMEL: LonLat = [-2.39, 47.93];

function run(start: Start, mode: TourMode): Run {
  return {
    name: 'solve-test',
    bbox: { west: -5.2, south: 47.2, east: -1.0, north: 48.95 },
    start,
    mode,
    video: DEFAULT_VIDEO,
    overpass: null,
  };
}

function haversine(a: LonLat, b: LonLat): number {
  const rad = Math.PI / 180;
  const dLat = (b[1] - a[1]) * rad;
  const dLon = (b[0] - a[0]) * rad;
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(a[1] * rad) * Math.cos(b[1] * rad) * Math.sin(dLon / 2) ** 2;
  return 2 * 6371008.8 * Math.asin(Math.sqrt(h));
}

function segmentLengths(points: readonly LonLat[]): number[] {
  return points.flatMap((point, i) => {
    const previous = points[i - 1];
    return previous === undefined ? [] : [haversine(previous, point)];
  });
}

function lengthOf(points: readonly LonLat[]): number {
  return segmentLengths(points).reduce((sum, meters) => sum + meters, 0);
}

function decreases(values: readonly number[]): number[] {
  return values.flatMap((value, i) => {
    const previous = values[i - 1];
    return previous !== undefined && value < previous ? [i] : [];
  });
}

function field(value: unknown, key: string): unknown {
  return typeof value === 'object' && value !== null && key in value ? Reflect.get(value, key) : undefined;
}

function coordinatesOf(url: string): LonLat[] {
  const last = new URL(url).pathname.split('/').at(-1) ?? '';
  return decodeURIComponent(last)
    .split(';')
    .map((pair) => {
      const [lon = Number.NaN, lat = Number.NaN] = pair.split(',').map(Number);
      return [lon, lat] as const;
    });
}

function jsonResponse(body: unknown): Promise<Response> {
  return Promise.resolve(Response.json(body));
}

/** VROOM visite d'abord le job situé au départ du véhicule, puis les autres dans l'ordre de la requête. */
function vroomSolution(request: unknown): unknown {
  const rawJobs = field(request, 'jobs');
  const jobs = Array.isArray(rawJobs)
    ? rawJobs.flatMap((job: unknown) => {
        const id = field(job, 'id');
        const location = field(job, 'location_index');
        return typeof id === 'number' && typeof location === 'number' ? [{ id, location }] : [];
      })
    : [];
  const start = field(field(field(request, 'vehicles'), '0'), 'start_index');
  const ordered = [...jobs.filter((job) => job.location === start), ...jobs.filter((job) => job.location !== start)];
  return {
    code: 0,
    routes: [{ steps: [{ type: 'start' }, ...ordered.map((job) => ({ type: 'job', id: job.id })), { type: 'end' }] }],
    unassigned: [],
  };
}

/**
 * Faux OSRM + VROOM : distances haversine, aucune route vers ou depuis les points `unreachable`,
 * et les points `snappedAway` projetés à 5 km sur le réseau.
 */
function fakeRouting(unreachable: readonly LonLat[] = [], snappedAway: readonly LonLat[] = []) {
  const isUnreachable = (point: LonLat) => unreachable.some((other) => isDeepStrictEqual(other, point));
  const isSnappedAway = (point: LonLat) => snappedAway.some((other) => isDeepStrictEqual(other, point));
  return (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    if (url.startsWith(VROOM_URL)) {
      const body: unknown = typeof init?.body === 'string' ? JSON.parse(init.body) : null;
      return jsonResponse(vroomSolution(body));
    }
    if (url.includes('/table/')) {
      const locations = coordinatesOf(url);
      const distances = locations.map((from, i) =>
        locations.map((to, j) => {
          if (i === j) return 0;
          return isUnreachable(from) || isUnreachable(to) ? null : haversine(from, to);
        }),
      );
      const sources = locations.map((point) => ({ distance: isSnappedAway(point) ? 5000 : 3 }));
      return jsonResponse({ code: 'Ok', distances, sources });
    }
    if (url.includes('/route/')) {
      const waypoints = coordinatesOf(url);
      const legs = segmentLengths(waypoints);
      return jsonResponse({
        code: 'Ok',
        routes: [
          {
            distance: lengthOf(waypoints),
            geometry: { type: 'LineString', coordinates: waypoints },
            legs: legs.map((meters) => ({ annotation: { distance: [meters] } })),
          },
        ],
      });
    }
    return Promise.resolve(new Response('unexpected request', { status: 404 }));
  };
}

function loopRoute(meters: (length: number) => number): RoutedPath {
  const points: NonEmpty<LonLat> = [RENNES.at, BREST.at, VANNES.at, RENNES.at];
  return { points, legSegments: [1, 1, 1], ferrySegments: [], meters: meters(lengthOf(points)) };
}

describe('findUnreachable', () => {
  test('finds no unreachable place in a fully connected matrix', () => {
    const matrix = [
      [0, 5, 7],
      [5, 0, 4],
      [7, 4, 0],
    ];

    assert.deepEqual(findUnreachable(matrix), []);
  });

  test('finds a place that no other place can reach', () => {
    const matrix = [
      [0, 5, 7, 3, null],
      [5, 0, 4, 6, null],
      [7, 4, 0, 2, null],
      [3, 6, 2, 0, null],
      [8, 9, 8, 7, 0],
    ];

    assert.deepEqual(findUnreachable(matrix), [4]);
  });

  test('finds a place that can reach no other place', () => {
    const matrix = [
      [0, 5, 7, 3, 8],
      [5, 0, 4, 6, 9],
      [7, 4, 0, 2, 8],
      [3, 6, 2, 0, 7],
      [null, null, null, null, 0],
    ];

    assert.deepEqual(findUnreachable(matrix), [4]);
  });
});

describe('routeWaypoints', () => {
  test('starts the waypoints at the start coordinates', () => {
    assert.deepEqual(routeWaypoints(PLOERMEL, [RENNES, BREST], 'open')[0], PLOERMEL);
  });

  test('ends the waypoints at the departure in loop mode', () => {
    assert.deepEqual(routeWaypoints(PLOERMEL, [RENNES, BREST], 'loop').at(-1), PLOERMEL);
  });

  test('ends the waypoints at the last place in open mode', () => {
    assert.deepEqual(routeWaypoints(PLOERMEL, [RENNES, BREST], 'open').at(-1), BREST.at);
  });

  test('routes from the departure to a single place and back in loop mode', () => {
    assert.deepEqual(routeWaypoints(PLOERMEL, [VANNES], 'loop'), [PLOERMEL, VANNES.at, PLOERMEL]);
  });

  test('routes from the departure to a single place in open mode', () => {
    assert.deepEqual(routeWaypoints(PLOERMEL, [VANNES], 'open'), [PLOERMEL, VANNES.at]);
  });
});

describe('assembleTour', () => {
  test('lists every place exactly once in the tour stops', () => {
    const tour = assembleTour({
      run: run({ kind: 'free' }, 'loop'),
      start: null,
      ordered: [RENNES, BREST, VANNES],
      route: loopRoute((length) => length),
      excluded: [],
    });

    assert.deepEqual(
      tour.stops.map((stop) => stop.id),
      ['a', 'b', 'c'],
    );
  });

  test('places the stops at non-decreasing distances', () => {
    const points: NonEmpty<LonLat> = [
      PLOERMEL,
      [-2.0, 48.0],
      RENNES.at,
      BREST.at,
      [-4.0, 48.0],
      [-3.5, 47.8],
      VANNES.at,
    ];
    const tour = assembleTour({
      run: run({ kind: 'coordinates', at: PLOERMEL }, 'open'),
      start: PLOERMEL,
      ordered: [RENNES, BREST, VANNES],
      route: { points, legSegments: [2, 1, 3], ferrySegments: [], meters: lengthOf(points) },
      excluded: [],
    });

    assert.deepEqual(decreases(tour.stops.map((stop) => stop.meters)), []);
  });

  test('measures the ferry crossings along the path', () => {
    const route = loopRoute((length) => length);
    const tour = assembleTour({
      run: run({ kind: 'free' }, 'loop'),
      start: null,
      ordered: [RENNES, BREST, VANNES],
      route: { ...route, ferrySegments: [[1, 2]] },
      excluded: [],
    });

    assert.deepEqual(tour.ferries, [[tour.path.meters[1], tour.path.meters[2]]]);
  });

  test('rejects a route whose geometry length differs from the OSRM distance by more than 1 %', () => {
    const input = {
      run: run({ kind: 'free' }, 'loop'),
      start: null,
      ordered: [RENNES, BREST, VANNES],
      route: loopRoute((length) => length * 1.02),
      excluded: [],
    } as const;

    assert.throws(() => assembleTour(input), RouteMismatchError);
  });

  test('accepts a route whose geometry length is within 1 % of the OSRM distance', () => {
    const input = {
      run: run({ kind: 'free' }, 'loop'),
      start: null,
      ordered: [RENNES, BREST, VANNES],
      route: loopRoute((length) => length * 1.005),
      excluded: [],
    } as const;

    assert.doesNotThrow(() => assembleTour(input));
  });
});

describe('solveTour', () => {
  test('lists the excluded places in the tour', async (t) => {
    t.mock.method(globalThis, 'fetch', fakeRouting([SEIN.at]));

    const tour = await solveTour(run({ kind: 'free' }, 'loop'), [RENNES, BREST, VANNES, SEIN], ENDPOINTS);

    assert.deepEqual(tour.excluded, [{ id: 'sein', name: 'Île de Sein' }]);
  });

  test('starts the tour at the start place', async (t) => {
    t.mock.method(globalThis, 'fetch', fakeRouting());

    const tour = await solveTour(run({ kind: 'place', id: 'c' }, 'loop'), [RENNES, BREST, VANNES], ENDPOINTS);

    assert.deepEqual(tour.path.points[0], VANNES.at);
  });

  test('rejects a start place id absent from the places', async (t) => {
    t.mock.method(globalThis, 'fetch', fakeRouting());

    await assert.rejects(
      solveTour(run({ kind: 'place', id: 'nowhere' }, 'loop'), [RENNES, BREST, VANNES], ENDPOINTS),
      UnknownStartPlaceError,
    );
  });

  test('starts the tour at a place when no start is given', async (t) => {
    t.mock.method(globalThis, 'fetch', fakeRouting());
    const places: NonEmpty<Place> = [RENNES, BREST, VANNES];

    const tour = await solveTour(run({ kind: 'free' }, 'open'), places, ENDPOINTS);

    assert.ok(places.some((place) => isDeepStrictEqual(place.at, tour.path.points[0])));
  });

  test('ends a loop without start at its first place', async (t) => {
    t.mock.method(globalThis, 'fetch', fakeRouting());

    const tour = await solveTour(run({ kind: 'free' }, 'loop'), [RENNES, BREST, VANNES], ENDPOINTS);

    assert.deepEqual(tour.path.points.at(-1), tour.path.points[0]);
  });

  test('computes the tour without the unreachable places', async (t) => {
    t.mock.method(globalThis, 'fetch', fakeRouting([SEIN.at]));

    const tour = await solveTour(run({ kind: 'free' }, 'loop'), [RENNES, BREST, VANNES, SEIN], ENDPOINTS);

    assert.deepEqual(tour.stops.map((stop) => stop.id).toSorted(), ['a', 'b', 'c']);
  });

  test('excludes a place that OSRM snaps far away from it', async (t) => {
    t.mock.method(globalThis, 'fetch', fakeRouting([], [SEIN.at]));

    const tour = await solveTour(run({ kind: 'free' }, 'loop'), [RENNES, BREST, VANNES, SEIN], ENDPOINTS);

    assert.deepEqual(tour.excluded, [{ id: 'sein', name: 'Île de Sein' }]);
  });

  test('rejects an unreachable start', async (t) => {
    const island: LonLat = [-5.1, 48.45];
    t.mock.method(globalThis, 'fetch', fakeRouting([island]));

    await assert.rejects(
      solveTour(run({ kind: 'coordinates', at: island }, 'loop'), [RENNES, BREST, VANNES], ENDPOINTS),
      StartUnreachableError,
    );
  });
});
