// Script jetable : tous les bars et pubs d'une commune, désignée par son code INSEE.
// `node sources/oneshot/bars-commune.ts runs/grenoble-bars.json 38185`  (Lyon : 69123)
// La source Overpass générique cible une zone ISO (pays, région) : une commune se désigne par son code INSEE.
import { loadRun } from '../../src/cli/shared.ts';
import { writeJson } from '../../src/files.ts';
import { distanceMeters } from '../../src/geo.ts';
import { describePlaceIssue, parsePlaces, toPlacesFile, type Place } from '../../src/places.ts';
import { runPaths } from '../../src/run.ts';
import { fetchOverpass, MERGE_RADIUS_METERS, parseOverpassResponse, placesFromElements } from '../overpass.ts';

const query = (insee: string): string => `[out:json][timeout:120];
area["ref:INSEE"="${insee}"]["admin_level"="8"]->.commune;
(nwr["amenity"="bar"](area.commune); nwr["amenity"="pub"](area.commune););
out center tags;`;

const [runFile, insee] = process.argv.slice(2);
if (runFile === undefined || insee === undefined || !/^\d[\dAB]\d{3}$/.test(insee)) {
  throw new Error('usage : bars-commune.ts <run.json> <code INSEE>');
}
const run = await loadRun(runFile);
if (run === null) process.exit(1);

const elements = parseOverpassResponse(await fetchOverpass(query(insee)));
// En centre-ville, deux bars distincts sont souvent à moins de 50 m : on ne fusionne que les homonymes.
const perElement = elements.map((element) => placesFromElements([element]));
const unnamed = perElement.flatMap((result) => result.unnamed);
const places = perElement
  .flatMap((result) => result.places)
  .reduce<Place[]>((kept, place) => {
    const twin = kept.some(
      (other) =>
        other.name.toLowerCase() === place.name.toLowerCase() &&
        distanceMeters(other.at, place.at) < MERGE_RADIUS_METERS,
    );
    return twin ? kept : [...kept, place];
  }, []);
if (unnamed.length > 0) console.warn(`${String(unnamed.length)} bars sans nom écartés : ${unnamed.join(', ')}`);
const file = toPlacesFile(places);
const validated = parsePlaces(JSON.stringify(file), run.bbox);
if (!validated.ok) {
  for (const issue of validated.errors) console.error(describePlaceIssue(issue));
  process.exit(1);
}
await writeJson(runPaths(run).places, file);
console.log(`${runPaths(run).places} : ${String(validated.value.length)} bars (${String(elements.length)} objets OSM)`);
