import { loadRun, usage } from '../src/cli/shared.ts';
import { writeJson } from '../src/files.ts';
import { distanceMeters, inBBox, type BBox, type LonLat } from '../src/geo.ts';
import { isFiniteNumber, isRecord } from '../src/narrow.ts';
import { describePlaceIssue, parsePlaces, toPlacesFile, type Place } from '../src/places.ts';
import { runPaths, type OverpassSource } from '../src/run.ts';

export const OVERPASS_URL = 'https://overpass-api.de/api/interpreter';

export const MERGE_RADIUS_METERS = 50;

/** Élément d'une réponse `out center` : les ways et relations n'ont que leur centre. */
export type OverpassElement =
  | {
      readonly type: 'node';
      readonly id: number;
      readonly lat: number;
      readonly lon: number;
      readonly tags: Readonly<Record<string, string>>;
    }
  | {
      readonly type: 'way' | 'relation';
      readonly id: number;
      readonly center: { readonly lat: number; readonly lon: number };
      readonly tags: Readonly<Record<string, string>>;
    };

export type OverpassPlaces = {
  readonly places: readonly Place[];
  /** `id` des lieux écartés faute de tag `name`. */
  readonly unnamed: readonly string[];
};

export class OverpassError extends Error {
  readonly reason: string;

  constructor(reason: string) {
    super(`Overpass : ${reason}`);
    this.reason = reason;
  }
}

export function buildOverpassQuery(source: OverpassSource, bbox: BBox): string {
  const isoKey = source.areaIso.includes('-') ? 'ISO3166-2' : 'ISO3166-1';
  const box = [bbox.south, bbox.west, bbox.north, bbox.east].join(',');
  return [
    '[out:json][timeout:300];',
    `area["${isoKey}"="${source.areaIso}"]->.zone;`,
    `nwr${source.filter}(area.zone)(${box});`,
    'out center tags;',
  ].join('\n');
}

export function parseOverpassResponse(raw: unknown): OverpassElement[] {
  if (!isRecord(raw) || !Array.isArray(raw['elements'])) throw new OverpassError('réponse sans tableau elements');
  const elements: readonly unknown[] = raw['elements'];
  return elements.map((element) => {
    if (!isRecord(element) || !isFiniteNumber(element['id'])) throw new OverpassError('élément sans id');
    const { type, id } = element;
    const tags = parseTags(element['tags']);
    if (type === 'node' && isFiniteNumber(element['lat']) && isFiniteNumber(element['lon'])) {
      return { type, id, lat: element['lat'], lon: element['lon'], tags };
    }
    const center = element['center'];
    if (
      (type === 'way' || type === 'relation') &&
      isRecord(center) &&
      isFiniteNumber(center['lat']) &&
      isFiniteNumber(center['lon'])
    ) {
      return { type, id, center: { lat: center['lat'], lon: center['lon'] }, tags };
    }
    throw new OverpassError(`élément ${String(type)}/${String(id)} sans position`);
  });
}

function parseTags(raw: unknown): Record<string, string> {
  if (!isRecord(raw)) return {};
  return Object.fromEntries(
    Object.entries(raw).filter((entry): entry is [string, string] => typeof entry[1] === 'string'),
  );
}

function position(element: OverpassElement): LonLat {
  return element.type === 'node' ? [element.lon, element.lat] : [element.center.lon, element.center.lat];
}

export function placesFromElements(elements: readonly OverpassElement[]): OverpassPlaces {
  const kept: OverpassElement[] = [];
  // ponytail: fusion en O(n²), passer à une grille au-delà de quelques milliers de lieux.
  for (const element of elements) {
    const twin = kept.findIndex((other) => distanceMeters(position(other), position(element)) < MERGE_RADIUS_METERS);
    const keptTwin = kept[twin];
    if (keptTwin === undefined) kept.push(element);
    else if (keptTwin.tags['name'] === undefined && element.tags['name'] !== undefined) kept[twin] = element;
  }
  const places: Place[] = [];
  const unnamed: string[] = [];
  for (const element of kept) {
    const id = `osm:${element.type}/${String(element.id)}`;
    const name = element.tags['name']?.trim();
    if (name === undefined || name === '') unnamed.push(id);
    else
      places.push({
        id,
        name,
        at: position(element),
        source: `https://www.openstreetmap.org/${element.type}/${String(element.id)}`,
      });
  }
  return { places, unnamed };
}

export async function fetchOverpass(query: string): Promise<unknown> {
  const response = await fetch(OVERPASS_URL, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded', 'user-agent': 'dijkstravel' },
    body: new URLSearchParams({ data: query }),
  });
  if (!response.ok) throw new OverpassError(`HTTP ${String(response.status)}`);
  const raw: unknown = await response.json();
  return raw;
}

/** `node sources/overpass.ts <run.json>` → `data/<run>/places.geojson`. */
export async function main(argv: readonly string[]): Promise<number> {
  const [runFile] = argv;
  if (runFile === undefined) return usage('source:overpass <run.json>');
  const run = await loadRun(runFile);
  if (run === null) return 1;
  if (run.overpass === null) {
    console.error(`${runFile} : aucune section overpass.`);
    return 1;
  }
  const elements = parseOverpassResponse(await fetchOverpass(buildOverpassQuery(run.overpass, run.bbox)));
  const { places, unnamed } = placesFromElements(elements);
  if (unnamed.length > 0) console.warn(`${String(unnamed.length)} lieux sans nom écartés : ${unnamed.join(', ')}`);
  // Un polygone qui chevauche la bbox peut avoir son centre en dehors.
  const inside = places.filter((place) => inBBox(place.at, run.bbox));
  const file = toPlacesFile(inside);
  const validated = parsePlaces(JSON.stringify(file), run.bbox);
  if (!validated.ok) {
    for (const issue of validated.errors) console.error(describePlaceIssue(issue));
    return 1;
  }
  const out = runPaths(run).places;
  await writeJson(out, file);
  console.log(`${out} : ${String(validated.value.length)} lieux (${String(elements.length)} éléments OSM).`);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
