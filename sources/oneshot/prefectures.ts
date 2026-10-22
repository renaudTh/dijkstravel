// Script jetable : les 96 préfectures de métropole (Corse comprise), en CSV pour `npm run source:csv`.
// `node sources/oneshot/prefectures.ts > data/prefectures/prefectures.csv`
//
// Départements et code de leur chef-lieu : Code officiel géographique de l'INSEE au 1er janvier 2026.
// Position : la mairie de la préfecture (geo.api.gouv.fr), plus parlante que le centre de la commune.
import { isRecord } from '../../src/narrow.ts';

const COG_DEPARTEMENTS = 'https://www.insee.fr/fr/statistiques/fichier/8740222/v_departement_2026.csv';
const BATCH = 10;

const csvField = (text: string): string => (/[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text);

type Departement = { readonly code: string; readonly nom: string; readonly chefLieu: string };

function departements(csv: string): Departement[] {
  const [header, ...rows] = csv.trim().split(/\r?\n/).map((line) => line.split(',').map((field) => field.replace(/^"|"$/g, '')));
  const column = (name: string): number => header?.indexOf(name) ?? -1;
  const [dep, chefLieu, nom] = [column('DEP'), column('CHEFLIEU'), column('LIBELLE')];
  return rows
    .map((row) => ({ code: row[dep] ?? '', nom: row[nom] ?? '', chefLieu: row[chefLieu] ?? '' }))
    // Outre-mer (97x) hors du réseau routier métropolitain.
    .filter((d) => d.code !== '' && !d.code.startsWith('97'));
}

async function prefecture(d: Departement): Promise<string> {
  const response = await fetch(`https://geo.api.gouv.fr/communes/${d.chefLieu}?fields=nom,mairie,centre&format=json`);
  const raw: unknown = await response.json();
  if (!isRecord(raw) || typeof raw['nom'] !== 'string') throw new Error(`${d.code} : commune ${d.chefLieu} introuvable`);
  const point = isRecord(raw['mairie']) ? raw['mairie'] : raw['centre'];
  const coordinates = isRecord(point) ? point['coordinates'] : null;
  if (!Array.isArray(coordinates) || typeof coordinates[0] !== 'number' || typeof coordinates[1] !== 'number') {
    throw new Error(`${d.code} : pas de position pour ${raw['nom']}`);
  }
  const name = `${raw['nom']} · ${d.nom} (${d.code})`;
  return [d.code, csvField(name), String(coordinates[1]), String(coordinates[0]), COG_DEPARTEMENTS].join(',');
}

const list = departements(await (await fetch(COG_DEPARTEMENTS)).text());
if (list.length !== 96) throw new Error(`${String(list.length)} départements de métropole au lieu de 96`);
const lines: string[] = [];
// Par lots, pour rester sous la limite de débit de geo.api.gouv.fr.
for (let i = 0; i < list.length; i += BATCH) lines.push(...(await Promise.all(list.slice(i, i + BATCH).map(prefecture))));
console.log(['id,name,lat,lon,source', ...lines].join('\n'));
