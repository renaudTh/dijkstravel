import type { LonLat, MeasuredPath } from '../geo.ts';
import { isFiniteNumber, isLonLatArray, isNonEmptyString, isRecord } from '../narrow.ts';
import type { TourMode } from '../run.ts';

export type Stop = {
  readonly id: string;
  readonly name: string;
  readonly at: LonLat;
  /** Distance le long de `Tour.path` à l'arrivée sur ce lieu. */
  readonly meters: number;
};

export type ExcludedPlace = { readonly id: string; readonly name: string };

/** Contenu de `tour.json` : la référence de toutes les vidéos du run. */
export type Tour = {
  readonly run: string;
  readonly mode: TourMode;
  readonly start: LonLat;
  readonly path: MeasuredPath;
  readonly stops: readonly Stop[];
  readonly excluded: readonly ExcludedPlace[];
  /** Distance annoncée par OSRM, pour contrôler `path` (CA-26). */
  readonly routedMeters: number;
};

export class InvalidTourError extends Error {
  readonly reason: string;

  constructor(reason: string) {
    super(`tour.json invalide : ${reason}`);
    this.reason = reason;
  }
}

export function parseTour(raw: unknown): Tour {
  if (!isRecord(raw)) throw new InvalidTourError('racine');
  const { run, mode, start, path, stops, excluded, routedMeters } = raw;
  if (!isNonEmptyString(run)) throw new InvalidTourError('run');
  if (mode !== 'loop' && mode !== 'open') throw new InvalidTourError('mode');
  if (!isLonLatArray(start)) throw new InvalidTourError('start');
  if (!isFiniteNumber(routedMeters)) throw new InvalidTourError('routedMeters');
  return {
    run,
    mode,
    start,
    path: parsePath(path),
    stops: arrayOf(stops, 'stops').map(parseStop),
    excluded: arrayOf(excluded, 'excluded').map(parseExcluded),
    routedMeters,
  };
}

function arrayOf(raw: unknown, field: string): readonly unknown[] {
  if (!Array.isArray(raw)) throw new InvalidTourError(field);
  const items: readonly unknown[] = raw;
  return items;
}

function parsePath(raw: unknown): MeasuredPath {
  if (!isRecord(raw)) throw new InvalidTourError('path');
  const points = arrayOf(raw['points'], 'path.points').filter(isLonLatArray);
  const meters = arrayOf(raw['meters'], 'path.meters').filter(isFiniteNumber);
  const [firstPoint, ...restPoints] = points;
  const [firstMeter, ...restMeters] = meters;
  if (firstPoint === undefined || firstMeter === undefined || points.length !== meters.length) {
    throw new InvalidTourError('path');
  }
  return { points: [firstPoint, ...restPoints], meters: [firstMeter, ...restMeters] };
}

function parseStop(raw: unknown): Stop {
  if (!isRecord(raw)) throw new InvalidTourError('stop');
  const { id, name, at, meters } = raw;
  if (!isNonEmptyString(id) || !isNonEmptyString(name) || !isLonLatArray(at) || !isFiniteNumber(meters)) {
    throw new InvalidTourError('stop');
  }
  return { id, name, at, meters };
}

function parseExcluded(raw: unknown): ExcludedPlace {
  if (!isRecord(raw) || !isNonEmptyString(raw['id']) || !isNonEmptyString(raw['name'])) {
    throw new InvalidTourError('excluded');
  }
  return { id: raw['id'], name: raw['name'] };
}
