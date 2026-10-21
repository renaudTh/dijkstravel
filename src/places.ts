import { inBBox, type BBox, type LonLat, type NonEmpty } from './geo.ts';
import { isFiniteNumber, isNonEmptyString, isRecord } from './narrow.ts';
import type { Result } from './result.ts';

export type Place = {
  readonly id: string;
  readonly name: string;
  readonly at: LonLat;
  readonly source: string | null;
};

/** Le format figé de `places.geojson`, contrat avec toutes les sources. */
export type PlacesFile = {
  readonly type: 'FeatureCollection';
  readonly features: readonly PlaceFeature[];
};

export type PlaceFeature = {
  readonly type: 'Feature';
  readonly geometry: { readonly type: 'Point'; readonly coordinates: LonLat };
  readonly properties: { readonly id: string; readonly name: string; readonly source?: string };
};

export type PlaceIssue =
  | { readonly kind: 'not-json' }
  | { readonly kind: 'not-feature-collection' }
  | { readonly kind: 'empty' }
  | { readonly kind: 'not-a-point'; readonly index: number; readonly id: string | null }
  | { readonly kind: 'missing-id'; readonly index: number }
  | { readonly kind: 'duplicate-id'; readonly id: string; readonly indexes: readonly [number, number] }
  | { readonly kind: 'missing-name'; readonly index: number; readonly id: string }
  | { readonly kind: 'invalid-coordinates'; readonly index: number; readonly id: string | null }
  | { readonly kind: 'outside-bbox'; readonly index: number; readonly id: string | null; readonly swapped: boolean };

export function parsePlaces(text: string, bbox: BBox): Result<NonEmpty<Place>, PlaceIssue> {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return { ok: false, errors: [{ kind: 'not-json' }] };
  }
  if (!isRecord(raw) || raw['type'] !== 'FeatureCollection' || !Array.isArray(raw['features'])) {
    return { ok: false, errors: [{ kind: 'not-feature-collection' }] };
  }
  const features: readonly unknown[] = raw['features'];
  const issues: PlaceIssue[] = [];
  const places: Place[] = [];
  const firstIndexById = new Map<string, number>();
  features.forEach((feature, index) => {
    const place = parseFeature(feature, index, bbox, issues);
    if (place === null) return;
    const firstIndex = firstIndexById.get(place.id);
    if (firstIndex !== undefined) {
      issues.push({ kind: 'duplicate-id', id: place.id, indexes: [firstIndex, index] });
      return;
    }
    firstIndexById.set(place.id, index);
    places.push(place);
  });
  const [first, ...rest] = places;
  if (issues.length > 0) return { ok: false, errors: issues };
  if (first === undefined) return { ok: false, errors: [{ kind: 'empty' }] };
  return { ok: true, value: [first, ...rest] };
}

/** Ajoute les problèmes de la feature à `issues` ; renvoie `null` si elle en a. */
function parseFeature(feature: unknown, index: number, bbox: BBox, issues: PlaceIssue[]): Place | null {
  const properties = isRecord(feature) && isRecord(feature['properties']) ? feature['properties'] : {};
  const rawId = properties['id'];
  const id = isNonEmptyString(rawId) ? rawId : null;
  const name = properties['name'];
  const source = properties['source'];
  const geometry = isRecord(feature) ? feature['geometry'] : undefined;
  const before = issues.length;

  if (id === null) issues.push({ kind: 'missing-id', index });
  else if (!isNonEmptyString(name)) issues.push({ kind: 'missing-name', index, id });

  let at: LonLat | null = null;
  if (!isRecord(geometry) || geometry['type'] !== 'Point') {
    issues.push({ kind: 'not-a-point', index, id });
  } else {
    at = parseCoordinates(geometry['coordinates']);
    if (at === null) issues.push({ kind: 'invalid-coordinates', index, id });
    else if (!inBBox(at, bbox)) {
      issues.push({ kind: 'outside-bbox', index, id, swapped: inBBox([at[1], at[0]], bbox) });
    }
  }

  if (issues.length > before || id === null || !isNonEmptyString(name) || at === null) return null;
  return { id, name: name.trim(), at, source: typeof source === 'string' ? source : null };
}

function parseCoordinates(raw: unknown): LonLat | null {
  if (!Array.isArray(raw) || raw.length < 2) return null;
  const lon: unknown = raw[0];
  const lat: unknown = raw[1];
  if (!isFiniteNumber(lon) || !isFiniteNumber(lat)) return null;
  if (lon < -180 || lon > 180 || lat < -90 || lat > 90) return null;
  return [lon, lat];
}

export function toPlacesFile(places: readonly Place[]): PlacesFile {
  return {
    type: 'FeatureCollection',
    features: places.map((place) => ({
      type: 'Feature',
      geometry: { type: 'Point', coordinates: place.at },
      properties:
        place.source === null
          ? { id: place.id, name: place.name }
          : { id: place.id, name: place.name, source: place.source },
    })),
  };
}

export function describePlaceIssue(issue: PlaceIssue): string {
  switch (issue.kind) {
    case 'not-json':
      return "Le fichier n'est pas du JSON valide.";
    case 'not-feature-collection':
      return "La racine n'est pas un FeatureCollection avec un tableau features.";
    case 'empty':
      return 'Le fichier ne contient aucun lieu.';
    case 'not-a-point':
      return `${featureLabel(issue.index, issue.id)} : la géométrie doit être un Point.`;
    case 'missing-id':
      return `${featureLabel(issue.index, null)} : id manquant ou vide.`;
    case 'duplicate-id':
      return `id "${issue.id}" dupliqué aux features #${String(issue.indexes[0])} et #${String(issue.indexes[1])}.`;
    case 'missing-name':
      return `${featureLabel(issue.index, issue.id)} : name manquant ou vide.`;
    case 'invalid-coordinates':
      return `${featureLabel(issue.index, issue.id)} : coordonnées invalides, attendu [lon, lat] en degrés.`;
    case 'outside-bbox':
      return `${featureLabel(issue.index, issue.id)} : hors de la zone du run${
        issue.swapped ? ' (latitude et longitude probablement inversées)' : ''
      }.`;
  }
}

function featureLabel(index: number, id: string | null): string {
  return id === null ? `Feature #${String(index)}` : `Feature #${String(index)} (id "${id}")`;
}
