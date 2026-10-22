// Script jetable : blason de chaque club, image principale de son article Wikipédia (anglais).
// `node sources/oneshot/blasons.ts ligue1`  →  data/ligue1-stades/images/<id du lieu>.<svg|png>
//
// ⚠ Logos non libres pour la plupart (marques déposées, fair use limité à Wikipédia) : utilisés
// à la demande et sous la responsabilité de l'utilisateur (2026-10-22).
import { access, mkdir, writeFile } from 'node:fs/promises';
import { extname } from 'node:path';
import { isRecord } from '../../src/narrow.ts';

/** Id du lieu (celui de `stades.ts`) → titre de l'article du club. */
const ARTICLES: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  ligue1: {
    'angers-sco': 'Angers SCO',
    'aj-auxerre': 'AJ Auxerre',
    'stade-brestois': 'Stade Brestois 29',
    'le-havre-ac': 'Le Havre AC',
    'le-mans-fc': 'Le Mans FC',
    'rc-lens': 'RC Lens',
    'losc-lille': 'Lille OSC',
    'fc-lorient': 'FC Lorient',
    'olympique-lyonnais': 'Olympique Lyonnais',
    'olympique-de-marseille': 'Olympique de Marseille',
    'as-monaco': 'AS Monaco FC',
    'ogc-nice': 'OGC Nice',
    'paris-fc': 'Paris FC',
    'paris-saint-germain': 'Paris Saint-Germain FC',
    'stade-rennais': 'Stade Rennais FC',
    'rc-strasbourg-alsace': 'RC Strasbourg Alsace',
    'toulouse-fc': 'Toulouse FC',
    'estac-troyes': 'ES Troyes AC',
  },
  ligue2: {
    'fc-annecy': 'FC Annecy',
    'us-boulogne': 'US Boulogne',
    'clermont-foot-63': 'Clermont Foot',
    'dijon-fco': 'Dijon FCO',
    'usl-dunkerque': 'USL Dunkerque',
    'grenoble-foot-38': 'Grenoble Foot 38',
    'ea-guingamp': 'En Avant Guingamp',
    'stade-lavallois': 'Stade Lavallois',
    'fc-metz': 'FC Metz',
    'montpellier-hsc': 'Montpellier HSC',
    'as-nancy-lorraine': 'AS Nancy Lorraine',
    'fc-nantes': 'FC Nantes',
    'pau-fc': 'Pau FC',
    'red-star-fc': 'Red Star FC',
    'stade-de-reims': 'Stade de Reims',
    'rodez-af': 'Rodez AF',
    'as-saint-etienne': 'AS Saint-Étienne',
    'fc-sochaux-montbeliard': 'FC Sochaux-Montbéliard',
  },
};

const HEADERS = { 'user-agent': 'dijkstravel/0.1 (outil personnel de cartographie)' };

const league = process.argv[2] ?? '';
const articles = ARTICLES[league];
if (articles === undefined) throw new Error(`usage : blasons.ts ${Object.keys(ARTICLES).join(' | ')}`);

const params = new URLSearchParams({
  action: 'query',
  prop: 'pageimages',
  piprop: 'original',
  // Les blasons sont des fichiers non libres : sans `any`, l'API ne les renvoie pas.
  pilicense: 'any',
  titles: Object.values(articles).join('|'),
  redirects: '1',
  format: 'json',
  formatversion: '2',
});
const raw: unknown = await (await fetch(`https://en.wikipedia.org/w/api.php?${params.toString()}`, { headers: HEADERS })).json();
const query = isRecord(raw) && isRecord(raw['query']) ? raw['query'] : {};
const list = (value: unknown): readonly Readonly<Record<string, unknown>>[] => (Array.isArray(value) ? value.filter(isRecord) : []);
const renames = [...list(query['normalized']), ...list(query['redirects'])];
const resolve = (title: string): string =>
  renames.reduce((current, r) => (r['from'] === current && typeof r['to'] === 'string' ? r['to'] : current), title);
const imageOf = new Map(
  list(query['pages']).flatMap((page) => {
    const original = isRecord(page['original']) ? page['original']['source'] : undefined;
    return typeof page['title'] === 'string' && typeof original === 'string' ? [[page['title'], original] as const] : [];
  }),
);

const dir = `data/${league}-stades/images`;
await mkdir(dir, { recursive: true });
const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** upload.wikimedia.org répond 429 aux rafales : on attend ce qu'il demande (ou 15 s) et on recommence. */
async function download(url: string): Promise<Response> {
  for (let attempt = 1; ; attempt++) {
    const response = await fetch(url, { headers: HEADERS });
    if (response.status !== 429 || attempt === 6) return response;
    await sleep(Number(response.headers.get('retry-after') ?? '15') * 1000);
  }
}

// Séquentiel, et sans retélécharger les blasons déjà présents.
for (const [id, article] of Object.entries(articles)) {
  const url = imageOf.get(resolve(article));
  if (url === undefined) {
    console.warn(`${id} : pas d'image pour « ${article} »`);
    continue;
  }
  const file = `${dir}/${id}${extname(new URL(url).pathname).toLowerCase()}`;
  if (await access(file).then(() => true, () => false)) continue;
  const response = await download(url);
  if (!response.ok) {
    console.warn(`${id} : HTTP ${String(response.status)} pour ${url}`);
    continue;
  }
  await writeFile(file, Buffer.from(await response.arrayBuffer()));
  console.log(`${file}  ←  ${decodeURIComponent(new URL(url).pathname.split('/').pop() ?? '')}`);
  await sleep(2000);
}
