// Script jetable : les stades d'une saison de Ligue 1, 2 ou 3, Bundesliga, Premier League, Liga ou Serie A, en CSV pour `npm run source:csv`.
// `node sources/oneshot/stades.ts ligue1 > data/ligue1-stades.csv`
//
// Clubs et stades : tableaux « Stadiums and locations » de Wikipédia (2026–27 Ligue 1 / Ligue 2),
// relevés le 2026-10-22 ; Ligue 3 (ex-National) : tableau « Participants » de la saison 2026-2027 sur
// Wikipédia en français, plus à jour que l'anglais (Fleury à Bondoufle, Versailles à Saint-Germain-en-Laye),
// relevé le 2026-10-25. Coordonnées : celles des articles Wikipédia des stades, contrôlées par la
// commune que renvoie geo.api.gouv.fr (Monaco n'y figure pas : contrôle sauté). Bundesliga : tableau
// « Stadiums and locations » de 2026–27 Bundesliga, relevé le 2026-10-25, commune contrôlée par Nominatim ;
// de même pour la Premier League, la Liga et la Serie A (2026–27, relevés le 2026-10-25).

type Club = {
  readonly club: string;
  /** Nom affiché, en usage français, quand il diffère (Bayern Munich, FC Séville…) ; l'identifiant reste tiré de `club`. */
  readonly label?: string;
  readonly stadium: string;
  readonly article: string;
  readonly commune: string;
  /** Élément Wikidata, pour un stade sans article Wikipédia. */
  readonly wikidata?: string;
};

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
  // Articles de Wikipédia en français (cf. WIKIS).
  ligue3: [
    { club: 'Amiens SC', stadium: 'Stade de la Licorne', article: 'Stade de la Licorne', commune: 'Amiens' },
    { club: 'SC Aubagne Air Bel', stadium: 'Stade de Lattre-de-Tassigny', article: 'Stade de Lattre-de-Tassigny', commune: 'Aubagne' },
    { club: 'SC Bastia', stadium: 'Stade Armand-Cesari', article: 'Stade Armand-Cesari', commune: 'Furiani' },
    { club: 'Bourg-en-Bresse 01', stadium: 'Stade Marcel-Verchère', article: 'Stade Marcel-Verchère', commune: 'Bourg-en-Bresse' },
    { club: 'SM Caen', stadium: "Stade Michel-d'Ornano", article: "Stade Michel-d'Ornano", commune: 'Caen' },
    { club: 'AS Cannes', stadium: 'Stade Pierre-de-Coubertin', article: 'Stade Pierre-de-Coubertin (Cannes)', commune: 'Cannes' },
    { club: 'US Concarneau', stadium: 'Stade Guy-Piriou', article: 'Stade Guy-Piriou', commune: 'Concarneau' },
    { club: 'FC Fleury 91', stadium: 'Essonne Stadium', article: 'Essonne Stadium', commune: 'Bondoufle' },
    { club: 'VFC La Roche-sur-Yon', stadium: 'Stade Henri-Desgrange', article: 'Stade Henri-Desgrange', commune: 'La Roche-sur-Yon' },
    // Pas d'article Wikipédia ; stade sur la commune voisine du Puy, Wikidata et OSM d'accord.
    {
      club: 'Le Puy-en-Velay FC',
      stadium: 'Stade Charles-Massot',
      article: 'Stade Charles-Massot',
      commune: 'Espaly-Saint-Marcel',
      wikidata: 'Q122261810',
    },
    { club: 'US Orléans', stadium: 'Stade de la Source', article: 'Stade de la Source', commune: 'Orléans' },
    // Stade Pelé pour les cinq premières journées seulement : Charléty pour le reste de la saison.
    { club: 'Paris 13 Atletico', stadium: 'Stade Charléty', article: 'Stade Charléty', commune: 'Paris' },
    // Rouen et Quevilly-Rouen partagent Robert-Diochon : deux lieux au même point.
    { club: 'US Quevilly-Rouen', stadium: 'Stade Robert-Diochon', article: 'Stade Robert-Diochon', commune: 'Le Petit-Quevilly' },
    { club: 'FC Rouen', stadium: 'Stade Robert-Diochon', article: 'Stade Robert-Diochon', commune: 'Le Petit-Quevilly' },
    { club: 'US Thionville Lusitanos', stadium: 'Stade de Guentrange', article: 'Stade de Guentrange', commune: 'Thionville' },
    { club: 'Valenciennes FC', stadium: 'Stade du Hainaut', article: 'Stade du Hainaut', commune: 'Valenciennes' },
    { club: 'FC Versailles', stadium: 'Stade Georges-Lefèvre', article: 'Stade Georges-Lefèvre', commune: 'Saint-Germain-en-Laye' },
    { club: 'FC Villefranche Beaujolais', stadium: 'Stade Armand-Chouffet', article: 'Stade Armand-Chouffet', commune: 'Villefranche-sur-Saône' },
  ],
  // Noms d'usage allemands ; stades sous leur nom commercial actuel. Communes au sens de Nominatim.
  bundesliga: [
    { club: 'FC Augsburg', label: 'FC Augsbourg', stadium: 'WWK Arena', article: 'Augsburg Arena', commune: 'Augsburg' },
    { club: '1. FC Union Berlin', label: 'Union Berlin', stadium: 'An der Alten Försterei', article: 'Stadion An der Alten Försterei', commune: 'Berlin' },
    { club: 'SV Werder Bremen', label: 'Werder Brême', stadium: 'Weserstadion', article: 'Weserstadion', commune: 'Bremen' },
    { club: 'Borussia Dortmund', stadium: 'Signal Iduna Park', article: 'Westfalenstadion', commune: 'Dortmund' },
    { club: 'SV Elversberg', label: 'Elversberg', stadium: 'Ursapharm-Arena', article: 'Waldstadion an der Kaiserlinde', commune: 'Spiesen-Elversberg' },
    { club: 'Eintracht Frankfurt', label: 'Eintracht Francfort', stadium: 'Deutsche Bank Park', article: 'Waldstadion (Frankfurt)', commune: 'Frankfurt am Main' },
    { club: 'SC Freiburg', label: 'SC Fribourg', stadium: 'Europa-Park Stadion', article: 'Europa-Park Stadion', commune: 'Freiburg im Breisgau' },
    { club: 'Hamburger SV', label: 'Hambourg', stadium: 'Volksparkstadion', article: 'Volksparkstadion', commune: 'Hamburg' },
    { club: 'TSG Hoffenheim', label: 'Hoffenheim', stadium: 'PreZero Arena', article: 'Rhein-Neckar-Arena', commune: 'Sinsheim' },
    { club: '1. FC Köln', label: 'FC Cologne', stadium: 'RheinEnergieStadion', article: 'RheinEnergieStadion', commune: 'Köln' },
    { club: 'RB Leipzig', stadium: 'Red Bull Arena', article: 'Red Bull Arena (Leipzig)', commune: 'Leipzig' },
    { club: 'Bayer 04 Leverkusen', label: 'Bayer Leverkusen', stadium: 'BayArena', article: 'BayArena', commune: 'Leverkusen' },
    { club: '1. FSV Mainz 05', label: 'Mayence', stadium: 'Mewa Arena', article: 'Mewa Arena', commune: 'Mainz' },
    { club: 'Borussia Mönchengladbach', stadium: 'Borussia-Park', article: 'Borussia-Park', commune: 'Mönchengladbach' },
    { club: 'FC Bayern München', label: 'Bayern Munich', stadium: 'Allianz Arena', article: 'Allianz Arena', commune: 'München' },
    { club: 'SC Paderborn 07', label: 'Paderborn', stadium: 'Home Deluxe Arena', article: 'Home Deluxe Arena', commune: 'Paderborn' },
    { club: 'FC Schalke 04', label: 'Schalke 04', stadium: 'Veltins-Arena', article: 'Arena AufSchalke', commune: 'Gelsenkirchen' },
    { club: 'VfB Stuttgart', stadium: 'MHPArena', article: 'MHPArena', commune: 'Stuttgart' },
  ],
  // Communes au sens de Nominatim : à Londres, le borough ; ailleurs, parfois le district (Rushcliffe pour West Bridgford).
  premierleague: [
    { club: 'Arsenal', stadium: 'Emirates Stadium', article: 'Emirates Stadium', commune: 'London Borough of Islington' },
    { club: 'Aston Villa', stadium: 'Villa Park', article: 'Villa Park', commune: 'Birmingham' },
    { club: 'AFC Bournemouth', label: 'Bournemouth', stadium: 'Vitality Stadium', article: 'Dean Court', commune: 'Bournemouth, Christchurch and Poole' },
    { club: 'Brentford', stadium: 'Gtech Community Stadium', article: 'Brentford Community Stadium', commune: 'London Borough of Hounslow' },
    { club: 'Brighton & Hove Albion', label: 'Brighton', stadium: 'Amex Stadium', article: 'Falmer Stadium', commune: 'Brighton' },
    { club: 'Chelsea', stadium: 'Stamford Bridge', article: 'Stamford Bridge (stadium)', commune: 'London Borough of Hammersmith and Fulham' },
    { club: 'Coventry City', label: 'Coventry', stadium: 'Coventry Building Society Arena', article: 'Coventry Building Society Arena', commune: 'Coventry' },
    { club: 'Crystal Palace', stadium: 'Selhurst Park', article: 'Selhurst Park', commune: 'London Borough of Croydon' },
    { club: 'Everton', stadium: 'Hill Dickinson Stadium', article: 'Hill Dickinson Stadium', commune: 'Liverpool' },
    { club: 'Fulham', stadium: 'Craven Cottage', article: 'Craven Cottage', commune: 'London Borough of Hammersmith and Fulham' },
    { club: 'Hull City', stadium: 'MKM Stadium', article: 'MKM Stadium', commune: 'Hull' },
    { club: 'Ipswich Town', label: 'Ipswich', stadium: 'Portman Road', article: 'Portman Road', commune: 'Ipswich' },
    { club: 'Leeds United', label: 'Leeds', stadium: 'Elland Road', article: 'Elland Road', commune: 'Leeds' },
    { club: 'Liverpool', stadium: 'Anfield', article: 'Anfield', commune: 'Liverpool' },
    { club: 'Manchester City', stadium: 'Etihad Stadium', article: 'Etihad Stadium', commune: 'Manchester' },
    { club: 'Manchester United', stadium: 'Old Trafford', article: 'Old Trafford', commune: 'Trafford' },
    { club: 'Newcastle United', label: 'Newcastle', stadium: 'St James\' Park', article: 'St James\' Park', commune: 'Newcastle upon Tyne' },
    { club: 'Nottingham Forest', stadium: 'City Ground', article: 'City Ground', commune: 'Rushcliffe' },
    { club: 'Sunderland', stadium: 'Stadium of Light', article: 'Stadium of Light', commune: 'Sunderland' },
    { club: 'Tottenham Hotspur', label: 'Tottenham', stadium: 'Tottenham Hotspur Stadium', article: 'Tottenham Hotspur Stadium', commune: 'London Borough of Haringey' },
  ],
  laliga: [
    { club: 'Deportivo Alavés', label: 'Alavés', stadium: 'Mendizorrotza', article: 'Mendizorrotza Stadium', commune: 'Vitoria-Gasteiz' },
    { club: 'Athletic Club', label: 'Athletic Bilbao', stadium: 'San Mamés', article: 'San Mamés Stadium (2013)', commune: 'Bilbao' },
    { club: 'Atlético de Madrid', stadium: 'Riyadh Air Metropolitano', article: 'Metropolitano Stadium', commune: 'Madrid' },
    { club: 'FC Barcelona', label: 'FC Barcelone', stadium: 'Spotify Camp Nou', article: 'Camp Nou', commune: 'Barcelona' },
    { club: 'RC Celta', label: 'Celta de Vigo', stadium: 'Balaídos', article: 'Balaídos Stadium', commune: 'Vigo' },
    { club: 'RC Deportivo', label: 'Deportivo La Corogne', stadium: 'Riazor', article: 'Estadio Riazor', commune: 'A Coruña' },
    { club: 'Elche CF', label: 'Elche', stadium: 'Martínez Valero', article: 'Estadio Martínez Valero', commune: 'Elx / Elche' },
    // Stades à cheval sur deux communes : RCDE Stadium (Cornellà / El Prat), La Cartuja (Séville / Santiponce).
    { club: 'RCD Espanyol', label: 'Espanyol Barcelone', stadium: 'RCDE Stadium', article: 'RCDE Stadium', commune: 'el Prat de Llobregat' },
    { club: 'Getafe CF', label: 'Getafe', stadium: 'Coliseum', article: 'Estadio Coliseum', commune: 'Getafe' },
    { club: 'Levante UD', label: 'Levante', stadium: 'Ciutat de València', article: 'Estadi Ciutat de València', commune: 'València' },
    { club: 'Málaga CF', label: 'Málaga', stadium: 'La Rosaleda', article: 'Estadio La Rosaleda', commune: 'Málaga' },
    { club: 'CA Osasuna', label: 'Osasuna', stadium: 'El Sadar', article: 'El Sadar Stadium', commune: 'Pamplona / Iruña' },
    { club: 'Racing de Santander', label: 'Racing Santander', stadium: 'El Sardinero', article: 'Campos de Sport de El Sardinero', commune: 'Santander' },
    { club: 'Rayo Vallecano', stadium: 'Vallecas', article: 'Vallecas Stadium', commune: 'Madrid' },
    { club: 'Real Betis', stadium: 'La Cartuja', article: 'Estadio de La Cartuja', commune: 'Santiponce' },
    { club: 'Real Madrid', stadium: 'Santiago Bernabéu', article: 'Bernabéu (stadium)', commune: 'Madrid' },
    { club: 'Real Sociedad', stadium: 'Reale Arena', article: 'Anoeta Stadium', commune: 'Donostia / San Sebastián' },
    { club: 'Sevilla FC', label: 'FC Séville', stadium: 'Ramón Sánchez-Pizjuán', article: 'Ramón Sánchez-Pizjuán Stadium', commune: 'Sevilla' },
    { club: 'Valencia CF', label: 'Valence', stadium: 'Mestalla', article: 'Mestalla Stadium', commune: 'València' },
    { club: 'Villarreal CF', label: 'Villarreal', stadium: 'Estadio de la Cerámica', article: 'Estadio de la Cerámica', commune: 'Vila-real' },
  ],
  // Inter et Milan partagent San Siro, Lazio et Roma l'Olimpico : deux lieux au même point.
  seriea: [
    { club: 'Atalanta', label: 'Atalanta Bergame', stadium: 'New Balance Arena', article: 'Stadio di Bergamo', commune: 'Bergamo' },
    { club: 'Bologna', label: 'Bologne', stadium: 'Renato Dall\'Ara', article: 'Stadio Renato Dall\'Ara', commune: 'Bologna' },
    { club: 'Cagliari', stadium: 'Unipol Domus', article: 'Unipol Domus', commune: 'Casteddu/Cagliari' },
    { club: 'Como', label: 'Côme', stadium: 'Giuseppe Sinigaglia', article: 'Stadio Giuseppe Sinigaglia', commune: 'Como' },
    { club: 'Fiorentina', stadium: 'Artemio Franchi', article: 'Stadio Artemio Franchi', commune: 'Firenze' },
    { club: 'Frosinone', stadium: 'Benito Stirpe', article: 'Stadio Benito Stirpe', commune: 'Frosinone' },
    { club: 'Genoa', stadium: 'Luigi Ferraris', article: 'Stadio Luigi Ferraris', commune: 'Genova' },
    { club: 'Inter', label: 'Inter Milan', stadium: 'San Siro', article: 'San Siro', commune: 'Milano' },
    { club: 'Juventus', stadium: 'Allianz Stadium', article: 'Juventus Stadium', commune: 'Torino' },
    { club: 'Lazio', label: 'Lazio Rome', stadium: 'Stadio Olimpico', article: 'Stadio Olimpico', commune: 'Roma' },
    { club: 'Lecce', stadium: 'Via del Mare', article: 'Stadio Via del mare', commune: 'Lecce' },
    { club: 'Milan', label: 'AC Milan', stadium: 'San Siro', article: 'San Siro', commune: 'Milano' },
    { club: 'Monza', stadium: 'U-Power Stadium', article: 'Stadio Brianteo', commune: 'Monza' },
    { club: 'Napoli', label: 'Naples', stadium: 'Diego Armando Maradona', article: 'Stadio Diego Armando Maradona', commune: 'Napoli' },
    { club: 'Parma', label: 'Parme', stadium: 'Ennio Tardini', article: 'Stadio Ennio Tardini', commune: 'Parma' },
    { club: 'Roma', label: 'AS Rome', stadium: 'Stadio Olimpico', article: 'Stadio Olimpico', commune: 'Roma' },
    { club: 'Sassuolo', stadium: 'Mapei Stadium', article: 'Mapei Stadium – Città del Tricolore', commune: 'Reggio Emilia' },
    { club: 'Torino', stadium: 'Olimpico Grande Torino', article: 'Stadio Olimpico Grande Torino', commune: 'Torino' },
    { club: 'Udinese', stadium: 'Bluenergy Stadium', article: 'Stadio Friuli', commune: 'Udine' },
    { club: 'Venezia', label: 'Venise', stadium: 'Pier Luigi Penzo', article: 'Stadio Pier Luigi Penzo', commune: 'Venezia' },
  ],
};

/** Ligues hors de France : commune contrôlée par Nominatim, geo.api.gouv.fr ne connaissant que la France. */
const ABROAD = new Set(['bundesliga', 'premierleague', 'laliga', 'seriea']);

/** Édition de Wikipédia où chercher les articles des stades : l'anglaise sauf mention contraire. */
const WIKIS: Readonly<Record<string, string>> = { ligue3: 'fr' };

import { isFiniteNumber, isRecord } from '../../src/narrow.ts';

const USER_AGENT = 'dijkstravel/0.1 (outil personnel de cartographie)';

const slug = (text: string): string =>
  text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

/**
 * « Stade Geoffroy-Guichard » → « Geoffroy-Guichard », « Stade de la Meinau » → « La Meinau » : l'usage parle
 * du stade sans le mot « stade ». Gardés tels quels : les noms sans préfixe (Allianz Riviera, Parc des Princes…)
 * et ceux qui ne se disent pas sans lui.
 */
const KEEP_PREFIX = new Set(['Stade des Alpes', 'Stade de la Libération', 'Stade Océane', 'Stade de Lattre-de-Tassigny']);
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

async function coordinates(wiki: string, articles: readonly string[]): Promise<Map<string, LatLon>> {
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
  const response = await fetch(`https://${wiki}.wikipedia.org/w/api.php?${params.toString()}`, {
    headers: { 'user-agent': USER_AGENT },
  });
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

/** Nominatim, au niveau de la commune (`zoom=10`) ; une requête par seconde, d'après sa politique d'usage. */
async function municipalityAt(at: LatLon): Promise<string | null> {
  await new Promise((resolve) => setTimeout(resolve, 1100));
  const params = new URLSearchParams({ lat: String(at.lat), lon: String(at.lon), zoom: '10', format: 'jsonv2' });
  const response = await fetch(`https://nominatim.openstreetmap.org/reverse?${params.toString()}`, {
    headers: { 'user-agent': USER_AGENT },
  });
  const raw: unknown = await response.json();
  return isRecord(raw) && typeof raw['name'] === 'string' ? raw['name'] : null;
}

const league = process.argv[2] ?? '';
const clubs = LEAGUES[league];
if (clubs === undefined) throw new Error(`usage : stades.ts ${Object.keys(LEAGUES).join(' | ')}`);
const wiki = WIKIS[league] ?? 'en';
const found = await coordinates(wiki, clubs.flatMap((club) => (club.wikidata === undefined ? [club.article] : [])));
// Un club après l'autre : Nominatim n'accepte qu'une requête par seconde.
const located: (Club & LatLon)[] = [];
for (const club of clubs) {
  const at = club.wikidata === undefined ? found.get(club.article) : ((await wikidataCoordinates(club.wikidata)) ?? undefined);
  if (at === undefined) throw new Error(`Pas de coordonnées Wikipédia pour « ${club.article} »`);
  const commune = club.commune === 'Monaco' ? 'Monaco' : ABROAD.has(league) ? await municipalityAt(at) : await communeAt(at);
  if (commune !== club.commune) throw new Error(`${club.stadium} : coordonnées en « ${String(commune)} », attendu « ${club.commune} »`);
  located.push({ ...club, ...at });
}
console.log('id,name,lat,lon,source');
for (const club of located) {
  const name = `${shortStadium(club.stadium)} · ${club.label ?? club.club}`;
  const source =
    club.wikidata === undefined
      ? `https://${wiki}.wikipedia.org/wiki/${encodeURIComponent(club.article.replaceAll(' ', '_'))}`
      : `https://www.wikidata.org/wiki/${club.wikidata}`;
  console.log([slug(club.club), csvField(name), String(club.lat), String(club.lon), source].join(','));
}
