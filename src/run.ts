import type { BBox, LonLat } from './geo.ts';
import { isFiniteNumber, isLonLatArray, isNonEmptyString, isRecord } from './narrow.ts';
import type { Result } from './result.ts';

export const VIDEO_SIZES = {
  '16:9': { width: 1920, height: 1080 },
  '9:16': { width: 1080, height: 1920 },
} as const satisfies Record<string, { width: number; height: number }>;

export type VideoFormat = keyof typeof VIDEO_SIZES;

export type VideoSettings = {
  readonly format: VideoFormat;
  readonly durationSec: number;
  readonly introSec: number;
  readonly outroSec: number;
  readonly fps: number;
};

export const DEFAULT_VIDEO = {
  format: '16:9',
  durationSec: 60,
  introSec: 3,
  outroSec: 3,
  fps: 30,
} as const satisfies VideoSettings;

export type Start =
  | { readonly kind: 'free' }
  | { readonly kind: 'place'; readonly id: string }
  | { readonly kind: 'coordinates'; readonly at: LonLat };

export type TourMode = 'loop' | 'open';

export type OverpassSource = {
  /** Code ISO 3166-1 de la zone administrative, ex. `FR`. */
  readonly areaIso: string;
  /** Sélecteur de tags Overpass QL, ex. `["historic"="castle"]`. */
  readonly filter: string;
};

export type Run = {
  readonly name: string;
  readonly bbox: BBox;
  readonly start: Start;
  readonly mode: TourMode;
  readonly video: VideoSettings;
  readonly overpass: OverpassSource | null;
};

export type RunIssue = { readonly path: string; readonly expected: string };

export type RunPaths = {
  readonly dir: string;
  readonly places: string;
  readonly tour: string;
  readonly video: string;
};

export function parseRun(raw: unknown): Result<Run, RunIssue> {
  if (!isRecord(raw)) return { ok: false, errors: [{ path: '', expected: 'objet' }] };
  const issues: RunIssue[] = [];
  const name = raw['name'];
  const bbox = parseBBox(raw['bbox']);
  const start = parseStart(raw['start']);
  const mode = raw['mode'];
  const video = parseVideo(raw['video'] ?? {}, issues);
  const overpass = parseOverpass(raw['overpass']);
  if (!isNonEmptyString(name) || !/^[\w.-]+$/.test(name)) issues.push({ path: 'name', expected: 'nom de dossier' });
  if (bbox === null) issues.push({ path: 'bbox', expected: '{ west, south, east, north }' });
  if (start === null) issues.push({ path: 'start', expected: '{ place } | { lon, lat } | absent' });
  if (mode !== 'loop' && mode !== 'open') issues.push({ path: 'mode', expected: "'loop' | 'open'" });
  if (overpass === undefined) issues.push({ path: 'overpass', expected: '{ areaIso, filter } | absent' });
  if (
    issues.length > 0 ||
    !isNonEmptyString(name) ||
    bbox === null ||
    start === null ||
    (mode !== 'loop' && mode !== 'open') ||
    overpass === undefined ||
    video === null
  ) {
    return { ok: false, errors: issues };
  }
  return { ok: true, value: { name, bbox, start, mode, video, overpass } };
}

function parseBBox(raw: unknown): BBox | null {
  if (!isRecord(raw)) return null;
  const { west, south, east, north } = raw;
  if (!isFiniteNumber(west) || !isFiniteNumber(south) || !isFiniteNumber(east) || !isFiniteNumber(north)) return null;
  if (west >= east || south >= north) return null;
  return { west, south, east, north };
}

function parseStart(raw: unknown): Start | null {
  if (raw === undefined) return { kind: 'free' };
  if (!isRecord(raw)) return null;
  if (isNonEmptyString(raw['place'])) return { kind: 'place', id: raw['place'] };
  const at: unknown = [raw['lon'], raw['lat']];
  return isLonLatArray(at) ? { kind: 'coordinates', at } : null;
}

/** `undefined` : invalide ; `null` : absent. */
function parseOverpass(raw: unknown): OverpassSource | null | undefined {
  if (raw === undefined) return null;
  if (!isRecord(raw) || !isNonEmptyString(raw['areaIso']) || !isNonEmptyString(raw['filter'])) return undefined;
  return { areaIso: raw['areaIso'], filter: raw['filter'] };
}

function parseVideo(raw: unknown, issues: RunIssue[]): VideoSettings | null {
  if (!isRecord(raw)) {
    issues.push({ path: 'video', expected: 'objet' });
    return null;
  }
  const format = raw['format'] ?? DEFAULT_VIDEO.format;
  const durationSec = raw['durationSec'] ?? DEFAULT_VIDEO.durationSec;
  const introSec = raw['introSec'] ?? DEFAULT_VIDEO.introSec;
  const outroSec = raw['outroSec'] ?? DEFAULT_VIDEO.outroSec;
  const fps = raw['fps'] ?? DEFAULT_VIDEO.fps;
  const before = issues.length;
  if (format !== '16:9' && format !== '9:16') issues.push({ path: 'video.format', expected: "'16:9' | '9:16'" });
  if (!isFiniteNumber(durationSec) || durationSec <= 0)
    issues.push({ path: 'video.durationSec', expected: 'nombre > 0' });
  if (!isFiniteNumber(introSec) || introSec < 0) issues.push({ path: 'video.introSec', expected: 'nombre >= 0' });
  if (!isFiniteNumber(outroSec) || outroSec < 0) issues.push({ path: 'video.outroSec', expected: 'nombre >= 0' });
  if (!Number.isInteger(fps) || !isFiniteNumber(fps) || fps <= 0)
    issues.push({ path: 'video.fps', expected: 'entier > 0' });
  if (
    issues.length > before ||
    (format !== '16:9' && format !== '9:16') ||
    !isFiniteNumber(durationSec) ||
    !isFiniteNumber(introSec) ||
    !isFiniteNumber(outroSec) ||
    !isFiniteNumber(fps)
  ) {
    return null;
  }
  return { format, durationSec, introSec, outroSec, fps };
}

export function runPaths(run: Pick<Run, 'name'>): RunPaths {
  const dir = `data/${run.name}`;
  return { dir, places: `${dir}/places.geojson`, tour: `${dir}/tour.json`, video: `${dir}/video.mp4` };
}
