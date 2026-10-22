// Script jetable : toutes les gares de France desservies par des TER ferroviaires (pas les cars TER),
// sans l'Île-de-France sauf les gares de Paris intra-muros (grandes gares d'où partent des TER).
// `node sources/oneshot/gares-ter.ts data/gtfs-sncf/gtfs/stops.txt > data/gares-ter/gares.csv`
//
// Desserte : GTFS national SNCF (horaires-sncf, ressources.data.sncf.com), arrêts `StopPoint:OCETrain TER-<UIC>`.
// Commune de chaque gare : référentiel « Gares de voyageurs » (même portail), à défaut geo.api.gouv.fr.
import { readFile } from 'node:fs/promises';
import { isRecord } from '../../src/narrow.ts';

const GARES_DE_VOYAGEURS =
  'https://ressources.data.sncf.com/api/explore/v2.1/catalog/datasets/gares-de-voyageurs/exports/json';
const ILE_DE_FRANCE = new Set(['75', '77', '78', '91', '92', '93', '94', '95']);
/** Code UIC des gares françaises : les TER transfrontaliers desservent aussi la Suisse, l'Allemagne… */
const FRANCE_UIC = /^87\d{6}$/;

const csvField = (text: string): string => (/[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text);

/** Champs d'une ligne CSV, guillemets RFC 4180 compris. */
function fields(line: string): string[] {
  return [...line.matchAll(/(?:^|,)("(?:[^"]|"")*"|[^,]*)/g)].map((match) =>
    (match[1] ?? '').replace(/^"|"$/g, '').replaceAll('""', '"'),
  );
}

type Gare = { readonly uic: string; readonly name: string; readonly lat: string; readonly lon: string };

async function terStops(stopsFile: string): Promise<Gare[]> {
  const [header = '', ...lines] = (await readFile(stopsFile, 'utf8')).trim().split(/\r?\n/);
  const columns = fields(header);
  const at = (row: string[], name: string): string => row[columns.indexOf(name)] ?? '';
  const byUic = new Map<string, Gare>();
  for (const row of lines.map(fields)) {
    const uic = /^StopPoint:OCETrain TER-(\d+)$/.exec(at(row, 'stop_id'))?.[1];
    if (uic === undefined || !FRANCE_UIC.test(uic)) continue;
    byUic.set(uic, { uic, name: at(row, 'stop_name'), lat: at(row, 'stop_lat'), lon: at(row, 'stop_lon') });
  }
  return [...byUic.values()];
}

/** Code UIC → code INSEE de la commune, d'après le référentiel des gares de voyageurs. */
async function communes(): Promise<Map<string, string>> {
  const raw: unknown = await (await fetch(GARES_DE_VOYAGEURS)).json();
  const entries = (Array.isArray(raw) ? raw : []).filter(isRecord).flatMap((gare) => {
    const insee = gare['codeinsee'];
    const uics = typeof gare['codes_uic'] === 'string' ? gare['codes_uic'].split(/[;,\s]+/) : [];
    return typeof insee === 'string' ? uics.map((uic) => [uic, insee] as const) : [];
  });
  return new Map(entries);
}

async function inseeAt(gare: Gare): Promise<string | null> {
  const response = await fetch(`https://geo.api.gouv.fr/communes?lat=${gare.lat}&lon=${gare.lon}&fields=code&format=json`);
  const [first]: unknown[] = await response.json().then((raw: unknown) => (Array.isArray(raw) ? raw : []));
  return isRecord(first) && typeof first['code'] === 'string' ? first['code'] : null;
}

const department = (insee: string): string => (insee.startsWith('97') ? insee.slice(0, 3) : insee.slice(0, 2));
/** Paris : code 75056, ou ses arrondissements 75101 à 75120. */
const inParis = (insee: string): boolean => insee === '75056' || /^751\d\d$/.test(insee);

const [stopsFile] = process.argv.slice(2);
if (stopsFile === undefined) throw new Error('usage : gares-ter.ts <stops.txt du GTFS SNCF>');
const [gares, inseeByUic] = await Promise.all([terStops(stopsFile), communes()]);
const missing = gares.filter((gare) => !inseeByUic.has(gare.uic));
// Gares absentes du référentiel : commune retrouvée par leur position, en séquence (une poignée).
for (const gare of missing) {
  const insee = await inseeAt(gare);
  if (insee !== null) inseeByUic.set(gare.uic, insee);
}
// Aucune commune française : gare à l'étranger malgré son code UIC en 87 (ex. Bâle Saint-Jean).
const abroad = gares.filter((gare) => !inseeByUic.has(gare.uic));
console.error(`À l'étranger, écartées : ${abroad.map((gare) => gare.name).join(', ')}`);
const kept = gares.filter((gare) => {
  const insee = inseeByUic.get(gare.uic);
  return insee !== undefined && (!ILE_DE_FRANCE.has(department(insee)) || inParis(insee));
});
console.error(
  `${String(gares.length)} gares TER en France, ${String(missing.length)} hors référentiel, ` +
    `${String(gares.length - abroad.length - kept.length)} écartées en Île-de-France, ${String(kept.length)} gardées`,
);
console.log('id,name,lat,lon,source');
for (const gare of kept) console.log([gare.uic, csvField(gare.name), gare.lat, gare.lon, 'GTFS SNCF'].join(','));
