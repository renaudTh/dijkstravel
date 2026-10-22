// Script jetable : les stades d'une saison de Ligue 1 ou de Ligue 2, en CSV pour `npm run source:csv`.
// `node sources/oneshot/stades.ts ligue1 > data/ligue1-stades.csv`
//
// Clubs et stades : tableaux « Stadiums and locations » de Wikipédia (2026–27 Ligue 1 / Ligue 2),
// relevés le 2026-10-22. Coordonnées : celles des articles Wikipédia des stades, contrôlées par la
// commune que renvoie geo.api.gouv.fr (Monaco n'y figure pas : contrôle sauté).

type Club = { readonly club: string; readonly stadium: string; readonly article: string; readonly commune: string };

const LEAGUES: Readonly<Record<string, readonly Club[]>> = {
  ligue1: [
    { club: 'Angers SCO', stadium: 'Stade Raymond-Kopa', article: 'Stade Raymond Kopa', commune: 'Angers' },
    { club: 'AJ Auxerre', stadium: "Stade de l'Abbé-Deschamps", article: 'Stade Abbé-Deschamps', commune: 'Auxerre' },
    { club: 'Stade brestois', stadium: 'Stade Francis-Le Blé', article: 'Stade Francis-Le Blé', commune: 'Brest' },
    { club: 'Le Havre AC', stadium: 'Stade Océane', article: 'Stade Océane', commune: 'Le Havre' },
    { club: 'Le Mans FC', stadium: 'Stade Marie-Marvingt', article: 'Stade Marie-Marvingt', commune: 'Le Mans' },
    { club: 'RC Lens', stadium: 'Stade Bollaert-Delelis', article: 'Stade Bollaert-Delelis', commune: 'Lens' },
    { club: 'LOSC Lille', stadium: 'Stade Pierre-Mauroy', article: 'Stade Pierre-Mauroy', commune: "Villeneuve-d'Ascq" },
    { club: 'FC Lorient', stadium: 'Stade du Moustoir', article: 'Stade du Moustoir', commune: 'Lorient' },
    { club: 'Olympique lyonnais', stadium: 'Groupama Stadium', article: 'Parc Olympique Lyonnais', commune: 'Décines-Charpieu' },
    { club: 'Olympique de Marseille', stadium: 'Stade Vélodrome', article: 'Stade Vélodrome', commune: 'Marseille' },
    { club: 'AS Monaco', stadium: 'Stade Louis-II', article: 'Stade Louis II', commune: 'Monaco' },
    { club: 'OGC Nice', stadium: 'Allianz Riviera', article: 'Allianz Riviera', commune: 'Nice' },
    { club: 'Paris FC', stadium: 'Stade Jean-Bouin', article: 'Stade Jean-Bouin (Paris)', commune: 'Paris' },
    { club: 'Paris Saint-Germain', stadium: 'Parc des Princes', article: 'Parc des Princes', commune: 'Paris' },
    { club: 'Stade rennais', stadium: 'Roazhon Park', article: 'Roazhon Park', commune: 'Rennes' },
    { club: 'RC Strasbourg Alsace', stadium: 'Stade de la Meinau', article: 'Stade de la Meinau', commune: 'Strasbourg' },
    { club: 'Toulouse FC', stadium: 'Stadium de Toulouse', article: 'Stadium de Toulouse', commune: 'Toulouse' },
    { club: 'ESTAC Troyes', stadium: "Stade de l'Aube", article: "Stade de l'Aube", commune: 'Troyes' },
  ],
  ligue2: [
    { club: 'FC Annecy', stadium: 'Parc des Sports', article: 'Parc des Sports (Annecy)', commune: 'Annecy' },
    { club: 'US Boulogne', stadium: 'Stade de la Libération', article: 'Stade de la Libération', commune: 'Boulogne-sur-Mer' },
    { club: 'Clermont Foot 63', stadium: 'Stade Gabriel-Montpied', article: 'Stade Gabriel-Montpied', commune: 'Clermont-Ferrand' },
    { club: 'Dijon FCO', stadium: 'Stade Gaston-Gérard', article: 'Stade Gaston Gérard', commune: 'Dijon' },
    { club: 'USL Dunkerque', stadium: 'Stade Marcel-Tribut', article: 'Stade Marcel-Tribut', commune: 'Dunkerque' },
    { club: 'Grenoble Foot 38', stadium: 'Stade des Alpes', article: 'Stade des Alpes', commune: 'Grenoble' },
    { club: 'EA Guingamp', stadium: 'Stade du Roudourou', article: 'Stade de Roudourou', commune: 'Guingamp' },
    { club: 'Stade lavallois', stadium: 'Stade Francis-Le-Basser', article: 'Stade Francis Le Basser', commune: 'Laval' },
    { club: 'FC Metz', stadium: 'Stade Saint-Symphorien', article: 'Stade Saint-Symphorien', commune: 'Longeville-lès-Metz' },
    { club: 'Montpellier HSC', stadium: 'Stade de la Mosson', article: 'Stade de la Mosson', commune: 'Montpellier' },
    { club: 'AS Nancy Lorraine', stadium: 'Stade Marcel-Picot', article: 'Stade Marcel Picot', commune: 'Tomblaine' },
    { club: 'FC Nantes', stadium: 'Stade de la Beaujoire', article: 'Stade de la Beaujoire', commune: 'Nantes' },
    // Stade de Pau bâti sur la commune voisine : Wikipédia et OSM le placent tous deux à Bizanos.
    { club: 'Pau FC', stadium: 'Nouste Camp', article: 'Nouste Camp', commune: 'Bizanos' },
    { club: 'Red Star FC', stadium: 'Stade Bauer', article: 'Stade Bauer', commune: 'Saint-Ouen-sur-Seine' },
    { club: 'Stade de Reims', stadium: 'Stade Auguste-Delaune', article: 'Stade Auguste-Delaune', commune: 'Reims' },
    { club: 'Rodez AF', stadium: 'Stade Paul-Lignon', article: 'Stade Paul-Lignon', commune: 'Rodez' },
    { club: 'AS Saint-Étienne', stadium: 'Stade Geoffroy-Guichard', article: 'Stade Geoffroy-Guichard', commune: 'Saint-Étienne' },
    { club: 'FC Sochaux-Montbéliard', stadium: 'Stade Auguste-Bonal', article: 'Stade Auguste-Bonal', commune: 'Montbéliard' },
  ],
};

import { isFiniteNumber, isRecord } from '../../src/narrow.ts';

const WIKIPEDIA_API = 'https://en.wikipedia.org/w/api.php';
const USER_AGENT = 'dijkstravel/0.1 (outil personnel de cartographie)';

const slug = (text: string): string =>
  text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

/**
 * « Stade Geoffroy-Guichard » → « Geoffroy-Guichard », « Stade de la Meinau » → « La Meinau » : l'usage parle
 * du stade sans le mot « stade ». Gardés tels quels : les noms sans préfixe (Allianz Riviera, Parc des Princes…)
 * et ceux qui ne se disent pas sans lui.
 */
const KEEP_PREFIX = new Set(['Stade des Alpes', 'Stade de la Libération', 'Stade Océane']);
function shortStadium(stadium: string): string {
  if (KEEP_PREFIX.has(stadium)) return stadium;
  // « de l' » est collé au nom (« de l'Aube ») : pas d'espace après l'apostrophe.
  const article = /^Stade (de la |du |de l'|des |de )(.+)$/.exec(stadium);
  if (article) {
    const [, link = '', rest = ''] = article;
    const short = { 'de la ': 'La ', 'du ': 'Le ', "de l'": "L'", 'des ': 'Les ', 'de ': '' }[link] ?? '';
    return short + rest;
  }
  return stadium.replace(/^Stade /, '');
}

const csvField = (text: string): string => (/[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text);

type LatLon = { readonly lat: number; readonly lon: number };

const records = (raw: unknown): readonly Readonly<Record<string, unknown>>[] =>
  Array.isArray(raw) ? raw.filter(isRecord) : [];

async function coordinates(articles: readonly string[]): Promise<Map<string, LatLon>> {
  const params = new URLSearchParams({
    action: 'query',
    prop: 'coordinates|pageprops',
    ppprop: 'wikibase_item',
    colimit: 'max',
    titles: articles.join('|'),
    redirects: '1',
    format: 'json',
    formatversion: '2',
  });
  const response = await fetch(`${WIKIPEDIA_API}?${params.toString()}`, { headers: { 'user-agent': USER_AGENT } });
  const raw: unknown = await response.json();
  const query = isRecord(raw) && isRecord(raw['query']) ? raw['query'] : {};
  const renames = [...records(query['normalized']), ...records(query['redirects'])];
  const resolve = (title: string): string =>
    renames.reduce((current, rename) => (rename['from'] === current && typeof rename['to'] === 'string' ? rename['to'] : current), title);
  const byTitle = new Map<string, LatLon>();
  const pages = records(query['pages']);
  // Article sans coordonnées (ex. Parc des Princes en anglais) : celles de son élément Wikidata.
  const fallbacks = await Promise.all(
    pages.map(async (page) => {
      const [first] = records(page['coordinates']);
      if (first && isFiniteNumber(first['lat']) && isFiniteNumber(first['lon'])) {
        return { title: page['title'], at: { lat: first['lat'], lon: first['lon'] } };
      }
      const item = isRecord(page['pageprops']) ? page['pageprops']['wikibase_item'] : undefined;
      return { title: page['title'], at: typeof item === 'string' ? await wikidataCoordinates(item) : null };
    }),
  );
  for (const { title, at } of fallbacks) if (typeof title === 'string' && at !== null) byTitle.set(title, at);
  return new Map(articles.flatMap((article) => {
    const found = byTitle.get(resolve(article));
    return found ? [[article, found] as const] : [];
  }));
}

async function wikidataCoordinates(item: string): Promise<LatLon | null> {
  const response = await fetch(`https://www.wikidata.org/wiki/Special:EntityData/${item}.json`, {
    headers: { 'user-agent': USER_AGENT },
  });
  const raw: unknown = await response.json();
  const entities = isRecord(raw) && isRecord(raw['entities']) ? raw['entities'] : {};
  const entity = entities[item];
  const claims = isRecord(entity) && isRecord(entity['claims']) ? entity['claims'] : {};
  const [claim] = records(claims['P625']);
  const snak = isRecord(claim?.['mainsnak']) ? claim['mainsnak'] : {};
  const value = isRecord(snak['datavalue']) && isRecord(snak['datavalue']['value']) ? snak['datavalue']['value'] : {};
  return isFiniteNumber(value['latitude']) && isFiniteNumber(value['longitude'])
    ? { lat: value['latitude'], lon: value['longitude'] }
    : null;
}

async function communeAt(at: LatLon): Promise<string | null> {
  const response = await fetch(`https://geo.api.gouv.fr/communes?lat=${String(at.lat)}&lon=${String(at.lon)}&fields=nom&format=json`);
  const [first] = records(await response.json());
  return typeof first?.['nom'] === 'string' ? first['nom'] : null;
}

const league = process.argv[2] ?? '';
const clubs = LEAGUES[league];
if (clubs === undefined) throw new Error(`usage : stades.ts ${Object.keys(LEAGUES).join(' | ')}`);
const found = await coordinates(clubs.map((club) => club.article));
const located = await Promise.all(clubs.map(async (club) => {
  const at = found.get(club.article);
  if (at === undefined) throw new Error(`Pas de coordonnées Wikipédia pour « ${club.article} »`);
  const commune = club.commune === 'Monaco' ? 'Monaco' : await communeAt(at);
  if (commune !== club.commune) throw new Error(`${club.stadium} : coordonnées en « ${String(commune)} », attendu « ${club.commune} »`);
  return { ...club, ...at };
}));
console.log('id,name,lat,lon,source');
for (const club of located) {
  const name = `${shortStadium(club.stadium)} · ${club.club}`;
  const source = `https://en.wikipedia.org/wiki/${encodeURIComponent(club.article.replaceAll(' ', '_'))}`;
  console.log([slug(club.club), csvField(name), String(club.lat), String(club.lon), source].join(','));
}
