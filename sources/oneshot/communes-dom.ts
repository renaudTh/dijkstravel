// Script jetable : les communes d'un département d'outre-mer, en CSV pour `npm run source:csv`.
// `node sources/oneshot/communes-dom.ts reunion > data/reunion-communes.csv`
//
// Communes du département : geo.api.gouv.fr (Code officiel géographique). Position : la mairie, plus
// parlante que le centre de la commune, comme pour les préfectures.
import { isRecord } from '../../src/narrow.ts';

const DEPARTEMENTS: Readonly<Record<string, { readonly code: string; readonly communes: number }>> = {
  guadeloupe: { code: '971', communes: 32 },
  martinique: { code: '972', communes: 34 },
  guyane: { code: '973', communes: 22 },
  reunion: { code: '974', communes: 24 },
};

const csvField = (text: string): string => (/[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text);

const name = process.argv[2] ?? '';
const departement = DEPARTEMENTS[name];
if (departement === undefined) throw new Error(`usage : communes-dom.ts ${Object.keys(DEPARTEMENTS).join(' | ')}`);
const url = `https://geo.api.gouv.fr/departements/${departement.code}/communes?fields=nom,code,mairie,centre&format=json`;
const raw: unknown = await (await fetch(url)).json();
const communes = Array.isArray(raw) ? raw.filter(isRecord) : [];
if (communes.length !== departement.communes) {
  throw new Error(`${String(communes.length)} communes au lieu de ${String(departement.communes)}`);
}
const lines = communes.map((commune) => {
  const point = isRecord(commune['mairie']) ? commune['mairie'] : commune['centre'];
  const coordinates = isRecord(point) ? point['coordinates'] : null;
  if (typeof commune['nom'] !== 'string' || typeof commune['code'] !== 'string') throw new Error('commune sans nom ni code');
  if (!Array.isArray(coordinates) || typeof coordinates[0] !== 'number' || typeof coordinates[1] !== 'number') {
    throw new Error(`${commune['code']} : pas de position pour ${commune['nom']}`);
  }
  return [commune['code'], csvField(commune['nom']), String(coordinates[1]), String(coordinates[0]), csvField(url)].join(',');
});
console.log(['id,name,lat,lon,source', ...lines].join('\n'));
