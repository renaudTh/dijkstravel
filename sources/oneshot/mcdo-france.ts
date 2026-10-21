// Script jetable : tous les McDonald's de France métropolitaine.
// `node sources/oneshot/mcdo-france.ts runs/france-mcdo.json`
//
// 1. Overpass : tout objet OSM tagué brand:wikidata=Q38076 (McDonald's), y compris ceux sans `name`,
//    que la source Overpass générique écarterait (CA-21) alors que la marque suffit à les identifier.
// 2. geo.api.gouv.fr : commune de chaque restaurant, pour afficher « McDonald's Rennes » plutôt que
//    1 500 fois « McDonald's ».
import { loadRun } from '../../src/cli/shared.ts';
import { writeJson } from '../../src/files.ts';
import { inBBox } from '../../src/geo.ts';
import { describePlaceIssue, parsePlaces, toPlacesFile, type Place } from '../../src/places.ts';
import { runPaths } from '../../src/run.ts';
import { buildOverpassQuery, fetchOverpass, parseOverpassResponse, placesFromElements } from '../overpass.ts';

const COMMUNE_API = 'https://geo.api.gouv.fr/communes';
/** geo.api.gouv.fr tolère 50 requêtes/s ; on reste bien en dessous. */
const BATCH = 10;

async function commune(place: Place): Promise<string | null> {
  const [lon, lat] = place.at;
  const response = await fetch(`${COMMUNE_API}?lat=${String(lat)}&lon=${String(lon)}&fields=nom&format=json`);
  if (!response.ok) return null;
  const raw: unknown = await response.json();
  const first: unknown = Array.isArray(raw) ? raw[0] : null;
  return typeof first === 'object' && first !== null && 'nom' in first && typeof first.nom === 'string'
    ? first.nom
    : null;
}

const [runFile] = process.argv.slice(2);
if (runFile === undefined) throw new Error('usage : mcdo-france.ts <run.json>');
const run = await loadRun(runFile);
if (run?.overpass == null) throw new Error(`${runFile} : run invalide ou sans section overpass`);

const elements = parseOverpassResponse(await fetchOverpass(buildOverpassQuery(run.overpass, run.bbox)));
const named = elements.map((element) => ({ ...element, tags: { ...element.tags, name: "McDonald's" } }));
const places = placesFromElements(named).places.filter((place) => inBBox(place.at, run.bbox));
console.log(`${String(elements.length)} objets OSM → ${String(places.length)} restaurants après fusion à 50 m`);

const batches = Array.from({ length: Math.ceil(places.length / BATCH) }, (_, i) =>
  places.slice(i * BATCH, (i + 1) * BATCH),
);
const communes: (string | null)[] = [];
// Par lots successifs pour respecter la limite de débit de l'API ; parallèle à l'intérieur d'un lot.
for (const batch of batches) communes.push(...(await Promise.all(batch.map(commune))));
const located = places.map((place, i) => ({ ...place, name: `McDonald's ${communes[i] ?? ''}`.trim() }));
console.log(`${String(communes.filter((name) => name === null).length)} restaurants sans commune trouvée`);

const file = toPlacesFile(located);
const validated = parsePlaces(JSON.stringify(file), run.bbox);
if (!validated.ok) {
  for (const issue of validated.errors) console.error(describePlaceIssue(issue));
  process.exit(1);
}
await writeJson(runPaths(run).places, file);
console.log(`${runPaths(run).places} : ${String(validated.value.length)} lieux`);
