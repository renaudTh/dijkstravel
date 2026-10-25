// Script jetable : les 157 tribunaux judiciaires de métropole (Corse comprise), en CSV pour `npm run source:csv`.
// `node sources/oneshot/tribunaux-judiciaires.ts > data/tribunaux-judiciaires.csv`
//
// Source : Annuaire de l'administration (DILA, service-public.fr), fiches de type `tgi` (tribunal judiciaire,
// ex-TGI), données du ministère de la Justice. Position : celle de l'adresse du tribunal dans sa fiche.
// Écartés : outre-mer (codes 97x, 98x), dont les tribunaux de première instance des collectivités.
import { isRecord } from '../../src/narrow.ts';

const EXPORT =
  'https://api-lannuaire.service-public.fr/api/explore/v2.1/catalog/datasets/api-lannuaire-administration/exports/json';
const METROPOLE = 157;

const csvField = (text: string): string => (/[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text);

/** « Tribunal judiciaire du Mans » → « Le Mans », « … des Sables-d'Olonne » → « Les Sables-d'Olonne ». */
function seat(name: string): string {
  const match = /^Tribunal judiciaire (d'|de la |du |des |de )(.+)$/.exec(name);
  if (match === null) throw new Error(`Nom inattendu : ${name}`);
  const [, link = '', rest = ''] = match;
  return ({ 'de la ': 'La ', 'du ': 'Le ', 'des ': 'Les ' }[link] ?? '') + rest;
}

const params = new URLSearchParams({
  select: 'nom,code_insee_commune,adresse,url_service_public',
  where: 'pivot like "\\"tgi\\""',
});
const raw: unknown = await (await fetch(`${EXPORT}?${params.toString()}`)).json();
const records = (Array.isArray(raw) ? raw.filter(isRecord) : []).filter((record) => {
  const code = record['code_insee_commune'];
  return typeof code === 'string' && !code.startsWith('97') && !code.startsWith('98');
});
if (records.length !== METROPOLE) throw new Error(`${String(records.length)} tribunaux en métropole au lieu de ${String(METROPOLE)}`);

const lines = records.map((record) => {
  const name = String(record['nom']);
  const code = String(record['code_insee_commune']);
  const addresses: unknown = typeof record['adresse'] === 'string' ? JSON.parse(record['adresse']) : null;
  const address: unknown = Array.isArray(addresses) ? addresses[0] : null;
  const lat = isRecord(address) ? Number(address['latitude']) : NaN;
  const lon = isRecord(address) ? Number(address['longitude']) : NaN;
  if (!Number.isFinite(lat) || !Number.isFinite(lon)) throw new Error(`${name} : pas de position`);
  return [code, csvField(`${seat(name)} · Tribunal judiciaire`), String(lat), String(lon), String(record['url_service_public'])].join(',');
});
console.log(['id,name,lat,lon,source', ...lines.toSorted()].join('\n'));
