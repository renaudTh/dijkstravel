// Script jetable : raccorde à la route les bouts de ferry qui aboutissent sur un quai piéton (footway,
// pedestrian, steps…), en OPL à fusionner dans l'extrait routier avant `osrm-extract`. Sans ce
// raccord, le profil car ne peut pas débarquer et l'île (les Saintes, la Désirade…) reste injoignable.
//
//   osmium tags-filter <extrait>.osm.pbf w/route=ferry w/highway -o lignes.osm.pbf
//   osmium add-locations-to-ways lignes.osm.pbf -f opl -o lignes.opl
//   node sources/oneshot/debarcaderes.ts lignes.opl > debarcaderes.opl
//   osmium sort debarcaderes.opl -o debarcaderes.osm.pbf  (puis osmium merge avec l'extrait)
import { readFile } from 'node:fs/promises';
import { distanceMeters, type LonLat } from '../../src/geo.ts';

/** Voies où passe une voiture : celles auxquelles un débarcadère doit aboutir. */
const CAR_ROADS = new Set([
  'motorway',
  'trunk',
  'primary',
  'secondary',
  'tertiary',
  'unclassified',
  'residential',
  'living_street',
  'service',
  'road',
  'motorway_link',
  'trunk_link',
  'primary_link',
  'secondary_link',
  'tertiary_link',
]);
/** Au-delà, le quai ne mène à aucune route : pas de raccord inventé. */
const MAX_QUAY_METERS = 500;
/** Identifiants des voies ajoutées, très au-dessus de ceux d'OSM (et des lignes de pirogue). */
const FIRST_ID = 91_000_000_000;

const file = process.argv[2];
if (file === undefined) throw new Error('usage : debarcaderes.ts <lignes.opl>');
const positions = new Map<string, LonLat>();
const ferryEnds: string[] = [];
const roadNodes = new Set<string>();
for (const line of (await readFile(file, 'utf8')).split('\n')) {
  if (!line.startsWith('w')) continue;
  const fields = line.split(' ');
  const tags = fields.find((field) => field.startsWith('T'))?.slice(1).split(',') ?? [];
  const nodes = (fields.find((field) => field.startsWith('N'))?.slice(1) ?? '').split(',').flatMap((ref) => {
    const match = /^n(\d+)x(-?[\d.]+)y(-?[\d.]+)$/.exec(ref);
    if (match === null) return [];
    const [, id = '', lon = '', lat = ''] = match;
    positions.set(id, [Number(lon), Number(lat)]);
    return [id];
  });
  const highway = tags.find((tag) => tag.startsWith('highway='))?.slice('highway='.length);
  if (tags.includes('route=ferry')) ferryEnds.push(...[nodes[0], nodes.at(-1)].filter((id) => id !== undefined));
  else if (highway !== undefined && CAR_ROADS.has(highway)) for (const id of nodes) roadNodes.add(id);
}

const roads = [...roadNodes];
let nextId = FIRST_ID;
const lines: string[] = [];
for (const end of new Set(ferryEnds)) {
  if (roadNodes.has(end)) continue;
  const at = positions.get(end);
  if (at === undefined) continue;
  const nearest = roads.reduce(
    (best, id) => {
      const meters = distanceMeters(at, positions.get(id) ?? at);
      return meters < best.meters ? { id, meters } : best;
    },
    { id: '', meters: Infinity },
  );
  if (nearest.meters > MAX_QUAY_METERS) continue;
  lines.push(`w${String(nextId++)} v1 Thighway=service,service=driveway Nn${end},n${nearest.id}`);
  console.error(`n${end} → n${nearest.id} : ${nearest.meters.toFixed(0)} m`);
}
console.log(lines.join('\n'));
