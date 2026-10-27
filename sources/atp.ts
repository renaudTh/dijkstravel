// Les magasins d'une enseigne en France, d'après All The Places (CC0).
// `node sources/atp.ts runs/lidl.json data/atp/lidl.geojson`
// `node sources/atp.ts runs/ikea.json data/atp/ikea.geojson STORE,SMALL,SHOP`
//
// Fichier d'un collecteur : https://alltheplaces-data.openaddresses.io/runs/<run>/output/<collecteur>.geojson
// (dernière exécution : https://data.alltheplaces.xyz/runs/latest.json). Collectés sur les sites des enseignes,
// souvent plus complets qu'OSM pour les chaînes.
import { readFile } from 'node:fs/promises';
import { loadRun } from '../src/cli/shared.ts';
import { writeJson } from '../src/files.ts';
import { inBBox } from '../src/geo.ts';
import { isLonLatArray, isRecord } from '../src/narrow.ts';
import { describePlaceIssue, parsePlaces, toPlacesFile, type Place } from '../src/places.ts';
import { runPaths } from '../src/run.ts';

/** « BELFORT Altkirch » → « Belfort Altkirch » : les enseignes écrivent souvent les villes en capitales. */
const tidy = (text: string): string =>
  text
    .replace(/\p{Lu}{2,}/gu, (word) => word.charAt(0) + word.slice(1).toLowerCase())
    .replace(/\s+/g, ' ')
    .trim();

const [runFile, geojsonFile, typesArg] = process.argv.slice(2);
if (runFile === undefined || geojsonFile === undefined) {
  throw new Error('usage : enseigne-atp.ts <run.json> <collecteur.geojson> [types séparés par des virgules]');
}
const types = typesArg === undefined ? null : new Set(typesArg.split(','));
const run = await loadRun(runFile);
if (run === null) process.exit(1);

const raw: unknown = JSON.parse(await readFile(geojsonFile, 'utf8'));
const features = isRecord(raw) && Array.isArray(raw['features']) ? raw['features'].filter(isRecord) : [];
const places = new Map<string, Place>();
for (const feature of features) {
  const props = isRecord(feature['properties']) ? feature['properties'] : {};
  const geometry = isRecord(feature['geometry']) ? feature['geometry'] : {};
  const at = geometry['coordinates'];
  const country = typeof props['addr:country'] === 'string' ? props['addr:country'].toUpperCase() : '';
  if (country !== 'FR' || !isLonLatArray(at) || !inBBox(at, run.bbox)) continue;
  if (types !== null && !types.has(typeof props['store_type'] === 'string' ? props['store_type'] : '')) continue;
  const brand = typeof props['name'] === 'string' ? props['name'] : '';
  const branch = typeof props['branch'] === 'string' ? tidy(props['branch']) : '';
  const name = branch.toLowerCase().startsWith(brand.toLowerCase()) ? branch : `${brand} ${branch}`.trim();
  const ref = typeof props['ref'] === 'string' ? props['ref'] : `${String(at[0])},${String(at[1])}`;
  const spider = typeof props['@spider'] === 'string' ? props['@spider'] : 'atp';
  const id = `atp:${spider}:${ref}`;
  const source = typeof props['website'] === 'string' ? props['website'] : null;
  places.set(id, { id, name, at: [at[0], at[1]], source });
}
const file = toPlacesFile([...places.values()]);
const validated = parsePlaces(JSON.stringify(file), run.bbox);
if (!validated.ok) {
  for (const issue of validated.errors) console.error(describePlaceIssue(issue));
  process.exit(1);
}
await writeJson(runPaths(run).places, file);
console.log(
  `${runPaths(run).places} : ${String(validated.value.length)} magasins (${String(features.length)} dans le fichier)`,
);
