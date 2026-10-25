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
  ligue3: {
    'amiens-sc': 'Amiens SC',
    'sc-aubagne-air-bel': 'SC Aubagne Air Bel',
    'sc-bastia': 'SC Bastia',
    'bourg-en-bresse-01': 'Football Bourg-en-Bresse Péronnas 01',
    'sm-caen': 'Stade Malherbe Caen',
    'as-cannes': 'AS Cannes',
    'us-concarneau': 'US Concarneau',
    'fc-fleury-91': 'FC Fleury 91 (men)',
    'vfc-la-roche-sur-yon': 'VFC La Roche-sur-Yon',
    'le-puy-en-velay-fc': 'Le Puy-en-Velay FC',
    'us-orleans': 'US Orléans',
    'paris-13-atletico': 'Paris 13 Atletico',
    'us-quevilly-rouen': 'US Quevilly-Rouen Métropole',
    'fc-rouen': 'FC Rouen',
    'us-thionville-lusitanos': 'US Thionville Lusitanos',
    'valenciennes-fc': 'Valenciennes FC',
    'fc-versailles': 'FC Versailles 78',
    'fc-villefranche-beaujolais': 'FC Villefranche Beaujolais',
  },
  bundesliga: {
    'fc-augsburg': 'FC Augsburg',
    '1-fc-union-berlin': '1. FC Union Berlin',
    'sv-werder-bremen': 'SV Werder Bremen',
    'borussia-dortmund': 'Borussia Dortmund',
    'sv-elversberg': 'SV Elversberg',
    'eintracht-frankfurt': 'Eintracht Frankfurt',
    'sc-freiburg': 'SC Freiburg',
    'hamburger-sv': 'Hamburger SV',
    'tsg-hoffenheim': 'TSG 1899 Hoffenheim',
    '1-fc-koln': '1. FC Köln',
    'rb-leipzig': 'RB Leipzig',
    'bayer-04-leverkusen': 'Bayer 04 Leverkusen',
    '1-fsv-mainz-05': '1. FSV Mainz 05',
    'borussia-monchengladbach': 'Borussia Mönchengladbach',
    'fc-bayern-munchen': 'FC Bayern Munich',
    'sc-paderborn-07': 'SC Paderborn 07',
    'fc-schalke-04': 'FC Schalke 04',
    'vfb-stuttgart': 'VfB Stuttgart',
  },
  premierleague: {
    'arsenal': 'Arsenal F.C.',
    'aston-villa': 'Aston Villa F.C.',
    'afc-bournemouth': 'AFC Bournemouth',
    'brentford': 'Brentford F.C.',
    'brighton-hove-albion': 'Brighton & Hove Albion F.C.',
    'chelsea': 'Chelsea F.C.',
    'coventry-city': 'Coventry City F.C.',
    'crystal-palace': 'Crystal Palace F.C.',
    'everton': 'Everton F.C.',
    'fulham': 'Fulham F.C.',
    'hull-city': 'Hull City A.F.C.',
    'ipswich-town': 'Ipswich Town F.C.',
    'leeds-united': 'Leeds United F.C.',
    'liverpool': 'Liverpool F.C.',
    'manchester-city': 'Manchester City F.C.',
    'manchester-united': 'Manchester United F.C.',
    'newcastle-united': 'Newcastle United F.C.',
    'nottingham-forest': 'Nottingham Forest F.C.',
    'sunderland': 'Sunderland A.F.C.',
    'tottenham-hotspur': 'Tottenham Hotspur F.C.',
  },
  laliga: {
    'deportivo-alaves': 'Deportivo Alavés',
    'athletic-club': 'Athletic Bilbao',
    'atletico-de-madrid': 'Atlético Madrid',
    'fc-barcelona': 'FC Barcelona',
    'rc-celta': 'RC Celta de Vigo',
    'rc-deportivo': 'Deportivo de La Coruña',
    'elche-cf': 'Elche CF',
    'rcd-espanyol': 'RCD Espanyol',
    'getafe-cf': 'Getafe CF',
    'levante-ud': 'Levante UD',
    'malaga-cf': 'Málaga CF',
    'ca-osasuna': 'CA Osasuna',
    'racing-de-santander': 'Racing de Santander',
    'rayo-vallecano': 'Rayo Vallecano',
    'real-betis': 'Real Betis',
    'real-madrid': 'Real Madrid CF',
    'real-sociedad': 'Real Sociedad',
    'sevilla-fc': 'Sevilla FC',
    'valencia-cf': 'Valencia CF',
    'villarreal-cf': 'Villarreal CF',
  },
  seriea: {
    'atalanta': 'Atalanta BC',
    'bologna': 'Bologna FC 1909',
    'cagliari': 'Cagliari Calcio',
    'como': 'Como 1907',
    'fiorentina': 'ACF Fiorentina',
    'frosinone': 'Frosinone Calcio',
    'genoa': 'Genoa CFC',
    'inter': 'Inter Milan',
    'juventus': 'Juventus FC',
    'lazio': 'SS Lazio',
    'lecce': 'US Lecce',
    'milan': 'AC Milan',
    'monza': 'AC Monza',
    'napoli': 'SSC Napoli',
    'parma': 'Parma Calcio 1913',
    'roma': 'AS Roma',
    'sassuolo': 'US Sassuolo Calcio',
    'torino': 'Torino FC',
    'udinese': 'Udinese Calcio',
    'venezia': 'Venezia FC',
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
