// Script jetable : lignes de pirogue des fleuves de Guyane, absentes d'OSM, en OPL à fusionner dans
// l'extrait routier avant `osrm-extract` (elles y deviennent des traversées `route=ferry`).
//
//   osmium tags-filter guyane-latest.osm.pbf w/waterway=river w/highway -o gy-lignes.osm.pbf
//   osmium add-locations-to-ways gy-lignes.osm.pbf -f opl -o gy-lignes.opl
//   node sources/oneshot/pirogues-guyane.ts gy-lignes.opl > pirogues.opl
//   osmium sort pirogues.opl -o pirogues.osm.pbf  (puis osmium merge avec guyane-latest.osm.pbf)
//
// Chaque ligne suit le lit du fleuve (waterway=river d'OSM, plus court chemin entre les débarcadères),
// et chaque débarcadère est relié à la route la plus proche du bourg par une voie `service`.
// Saül et Saint-Élie ne sont desservies ni par la route ni par le fleuve (avion, piste privée) : aucune ligne.
import { readFile } from 'node:fs/promises';
import { distanceMeters, type LonLat } from '../../src/geo.ts';
import { isRecord } from '../../src/narrow.ts';

const RIVERS = new Set(['Maroni', 'Lawa', 'Rio Oiapoque', 'Rivière Ouanary']);
/** Lignes, d'un bourg à l'autre, par leur code INSEE : le Maroni puis le Lawa, l'Oyapock, la rivière Ouanary. */
const LINES: readonly (readonly [string, string])[] = [
  ['97311', '97360'], // Saint-Laurent-du-Maroni → Apatou
  ['97360', '97357'], // Apatou → Grand-Santi
  ['97357', '97362'], // Grand-Santi → Papaïchton
  ['97362', '97353'], // Papaïchton → Maripasoula
  ['97308', '97356'], // Saint-Georges → Camopi
  ['97308', '97314'], // Saint-Georges → Ouanary
];
/** Voies où passe une voiture : un débarcadère ne se raccorde pas à un sentier. */
const CAR_ROADS = new Set([
  'motorway', 'trunk', 'primary', 'secondary', 'tertiary', 'unclassified', 'residential', 'living_street', 'service', 'road',
  'motorway_link', 'trunk_link', 'primary_link', 'secondary_link', 'tertiary_link',
]);
/** Un bout de fleuve qui finit sans rejoindre un autre (confluence mal cartographiée) le rejoint en deçà. */
const MAX_GAP_METERS = 2500;
/** Au-delà, le bourg n'a pas de route près du fleuve : le débarcadère est relié directement à la mairie. */
const MAX_LANDING_ROAD_METERS = 3000;
/** Allure d'une pirogue de transport, pour la durée de la traversée (`duration`, lue par le profil car). */
const PIROGUE_KMH = 25;
/** Identifiants des objets ajoutés, très au-dessus de ceux d'OSM. */
const FIRST_ID = 90_000_000_000;

type Way = { readonly tags: ReadonlyMap<string, string>; readonly nodes: readonly string[] };

/** Décode `%20%` et consorts (échappement OPL). */
const unescape = (text: string): string =>
  text.replace(/%([0-9a-f]+)%/gi, (_, hex: string) => String.fromCodePoint(Number.parseInt(hex, 16)));
const escape = (text: string): string =>
  [...text].map((char) => (/[\s,=@%]/.test(char) ? `%${(char.codePointAt(0) ?? 0).toString(16)}%` : char)).join('');

const file = process.argv[2];
if (file === undefined) throw new Error('usage : pirogues-guyane.ts <lignes.opl>');
const positions = new Map<string, LonLat>();
const ways: Way[] = [];
for (const line of (await readFile(file, 'utf8')).split('\n')) {
  if (!line.startsWith('w')) continue;
  const fields = line.split(' ');
  const tagField = fields.find((field) => field.startsWith('T'))?.slice(1) ?? '';
  const nodeField = fields.find((field) => field.startsWith('N'))?.slice(1) ?? '';
  const tags = new Map(
    tagField
      .split(',')
      .filter((pair) => pair !== '')
      .map((pair) => {
        const [key = '', value = ''] = pair.split('=');
        return [unescape(key), unescape(value)] as const;
      }),
  );
  const nodes = nodeField.split(',').flatMap((ref) => {
    const match = /^n(\d+)x(-?[\d.]+)y(-?[\d.]+)$/.exec(ref);
    if (match === null) return [];
    const [, id = '', lon = '', lat = ''] = match;
    positions.set(id, [Number(lon), Number(lat)]);
    return [id];
  });
  ways.push({ tags, nodes });
}
const at = (id: string): LonLat => positions.get(id) ?? [0, 0];

// Réseau des fleuves : sommets = nœuds OSM, arêtes = segments des waterway=river retenus.
const rivers = ways.filter((way) => way.tags.get('waterway') === 'river' && RIVERS.has(way.tags.get('name') ?? ''));
const edges = new Map<string, Map<string, number>>();
const link = (a: string, b: string): void => {
  const meters = distanceMeters(at(a), at(b));
  edges.set(a, (edges.get(a) ?? new Map<string, number>()).set(b, meters));
  edges.set(b, (edges.get(b) ?? new Map<string, number>()).set(a, meters));
};
for (const river of rivers) river.nodes.forEach((node, i) => i > 0 && link(river.nodes[i - 1] ?? node, node));
const riverNodes = [...edges.keys()];
const nearest = (point: LonLat, candidates: readonly string[]): { id: string; meters: number } =>
  candidates.reduce(
    (best, id) => {
      const meters = distanceMeters(point, at(id));
      return meters < best.meters ? { id, meters } : best;
    },
    { id: '', meters: Infinity },
  );
// Bouts de fleuve isolés : raccordés au sommet le plus proche d'un autre tronçon.
const gaps: [string, string][] = [];
for (const river of rivers) {
  for (const end of [river.nodes[0], river.nodes.at(-1)]) {
    if (end === undefined || (edges.get(end)?.size ?? 0) > 1) continue;
    const others = riverNodes.filter((id) => !river.nodes.includes(id));
    const closest = nearest(at(end), others);
    if (closest.meters <= MAX_GAP_METERS) gaps.push([end, closest.id]);
  }
}
for (const [a, b] of gaps) link(a, b);

/** Plus court chemin le long des fleuves (Dijkstra, file triée : le réseau est petit). */
function riverPath(from: string, to: string): string[] {
  const distance = new Map([[from, 0]]);
  const previous = new Map<string, string>();
  const queue = [from];
  const done = new Set<string>();
  while (queue.length > 0) {
    queue.sort((a, b) => (distance.get(a) ?? Infinity) - (distance.get(b) ?? Infinity));
    const node = queue.shift() ?? from;
    if (node === to) break;
    if (done.has(node)) continue;
    done.add(node);
    for (const [next, meters] of edges.get(node) ?? []) {
      const candidate = (distance.get(node) ?? Infinity) + meters;
      if (candidate < (distance.get(next) ?? Infinity)) {
        distance.set(next, candidate);
        previous.set(next, node);
        queue.push(next);
      }
    }
  }
  if (!previous.has(to)) throw new Error(`Pas de chemin par le fleuve entre les nœuds ${from} et ${to}`);
  const path = [to];
  while (path[0] !== from) path.unshift(previous.get(path[0] ?? from) ?? from);
  return path;
}

// Mairies des bourgs desservis, comme pour la liste des communes.
const raw: unknown = await (await fetch('https://geo.api.gouv.fr/departements/973/communes?fields=nom,code,mairie&format=json')).json();
const towns = new Map(
  (Array.isArray(raw) ? raw.filter(isRecord) : []).flatMap((commune) => {
    const mairie = isRecord(commune['mairie']) ? commune['mairie']['coordinates'] : null;
    return typeof commune['code'] === 'string' && typeof commune['nom'] === 'string' && Array.isArray(mairie)
      ? [[commune['code'], { name: commune['nom'], at: [Number(mairie[0]), Number(mairie[1])] as const }] as const]
      : [];
  }),
);
const roadNodes = [
  ...new Set(ways.filter((way) => CAR_ROADS.has(way.tags.get('highway') ?? '')).flatMap((way) => way.nodes)),
];

let nextId = FIRST_ID;
const nodeLines: string[] = [];
const wayLines: string[] = [];
const wayLine = (tags: Readonly<Record<string, string>>, nodes: readonly string[]): void => {
  const tagText = Object.entries(tags)
    .map(([key, value]) => `${escape(key)}=${escape(value)}`)
    .join(',');
  wayLines.push(`w${String(nextId++)} v1 T${tagText} N${nodes.map((id) => `n${id}`).join(',')}`);
};

/** Débarcadère d'un bourg : sommet du fleuve le plus proche de la mairie, relié à la route ou à la mairie. */
const landings = new Map<string, string>();
function landing(code: string): string {
  const known = landings.get(code);
  if (known !== undefined) return known;
  const town = towns.get(code);
  if (town === undefined) throw new Error(`Commune ${code} introuvable`);
  const river = nearest(town.at, riverNodes);
  const road = nearest(at(river.id), roadNodes);
  let shore = road.id;
  if (road.meters > MAX_LANDING_ROAD_METERS) {
    shore = String(nextId++);
    nodeLines.push(`n${shore} v1 x${String(town.at[0])} y${String(town.at[1])}`);
    positions.set(shore, town.at);
  }
  wayLine({ highway: 'service', name: `Débarcadère de ${town.name}` }, [shore, river.id]);
  console.error(
    `${town.name} : fleuve à ${(river.meters / 1000).toFixed(1)} km de la mairie, ` +
      (shore === road.id ? `route à ${(road.meters / 1000).toFixed(1)} km` : 'relié à la mairie'),
  );
  landings.set(code, river.id);
  return river.id;
}

for (const [from, to] of LINES) {
  const nodes = riverPath(landing(from), landing(to));
  const meters = nodes.reduce((sum, id, i) => sum + (i > 0 ? distanceMeters(at(nodes[i - 1] ?? id), at(id)) : 0), 0);
  const minutes = Math.round((meters / 1000 / PIROGUE_KMH) * 60);
  const duration = `${String(Math.floor(minutes / 60)).padStart(2, '0')}:${String(minutes % 60).padStart(2, '0')}`;
  const name = `Pirogue ${towns.get(from)?.name ?? from} - ${towns.get(to)?.name ?? to}`;
  wayLine({ route: 'ferry', ferry: 'pirogue', motor_vehicle: 'yes', duration, name }, nodes);
  console.error(`${name} : ${(meters / 1000).toFixed(0)} km, ${duration}`);
}
console.log([...nodeLines, ...wayLines].join('\n'));
