// Côté navigateur. `?frame=<n>` affiche l'image n sans capture (CA-43).
import {
  Map as MapLibre,
  setNow,
  type ExpressionSpecification,
  type GeoJSONSource,
  type LayerSpecification,
  type StyleSpecification,
} from 'maplibre-gl';
import { decimate, pointAt, splitCrossings, totalMeters, type MeasuredPath } from '../geo.ts';
import { IMAGERY_SOURCES } from '../imagery.ts';
import { isRecord } from '../narrow.ts';
import { parseRun, runTitle, type VideoSettings } from '../run.ts';
import { parseTour, type Tour } from '../solve/tour.ts';
import { parseTheme, type Theme, type ThemeIssue } from '../theme.ts';
import { cameraAt, frameCount, hudAt, momentAt, timeline, type Moment } from './frame.ts';

declare global {
  interface Window {
    /** Résolue quand la carte est entièrement chargée (`idle`). */
    renderFrame: (frame: number) => Promise<void>;
    frameCount: number;
    /** Sources encore en chargement, pour le diagnostic d'une image qui ne se termine pas. */
    pendingSources: () => string[];
    /** Applique un thème à chaud (éditeur) ; renvoie ses problèmes, sans rien changer s'il en a. */
    applyTheme: (raw: unknown) => Promise<readonly ThemeIssue[]>;
  }
}

const BASEMAP = 'https://tiles.openfreemap.org/styles/liberty';
const KEPT_LAYERS = new Set(['boundary_2', 'boundary_3']);
const TERRARIUM = 'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png';
/** Durée de l'effet « pop » d'un lieu atteint, en secondes de vidéo. */
const POP_SECONDS = 0.6;
/** Part de l'outro au bout de laquelle les noms des lieux ont disparu. */
const LABEL_FADE_PROGRESS = 0.5;
const MAX_DRAWN_POINTS = 20_000;
/** En dessous, le nom deviendrait illisible : on le coupe plutôt. */
const MIN_NAME_SCALE = 0.55;

class PageLoadError extends Error {
  readonly what: string;

  constructor(what: string) {
    super(`Chargement impossible : ${what}`);
    this.what = what;
  }
}

async function fetchJson(url: string): Promise<unknown> {
  const response = await fetch(url);
  if (!response.ok) throw new PageLoadError(url);
  const raw: unknown = await response.json();
  return raw;
}

function element(id: string): HTMLElement {
  const found = document.getElementById(id);
  if (found === null) throw new PageLoadError(`#${id}`);
  return found;
}

const line = (
  coordinates: readonly (readonly [number, number])[],
  properties: GeoJSON.GeoJsonProperties = {},
): GeoJSON.Feature => ({
  type: 'Feature',
  properties,
  geometry: { type: 'LineString', coordinates: coordinates.map(([lon, lat]) => [lon, lat]) },
});

function source(map: MapLibre, id: string): GeoJSONSource {
  const found = map.getSource<GeoJSONSource>(id);
  if (found === undefined) throw new PageLoadError(`source ${id}`);
  return found;
}

/** Tailles pleines au zoom d'une région (Bretagne ≈ 9), réduites au quart au zoom d'un pays (France ≈ 5). */
const FULL_SIZE_ZOOM = 9;
const SMALL_SIZE_ZOOM = 5;
const SMALL_SIZE_RATIO = 0.25;

const byZoom = (size: number | ExpressionSpecification): ExpressionSpecification => [
  'interpolate',
  ['linear'],
  ['zoom'],
  SMALL_SIZE_ZOOM,
  ['*', SMALL_SIZE_RATIO, size],
  FULL_SIZE_ZOOM,
  size,
];

type Paint = Readonly<Record<string, unknown>>;

/** Propriétés de peinture de chaque couche réglable, pour la création comme pour l'application à chaud. */
function paints(theme: Theme): Readonly<Record<string, Paint>> {
  const { route, stops, basemap } = theme;
  const coreWidth = route.width * route.coreRatio;
  const visitedRadius: ExpressionSpecification = [
    '+',
    route.width * stops.visitedRatio,
    ['*', stops.popRadius, ['get', 'pop']],
  ];
  const border = { 'line-color': basemap.borderColor, 'line-opacity': basemap.borders ? 0.6 : 0 };
  // Traversées en bateau dans leur couleur, bandes comprises.
  const routeColor: ExpressionSpecification = ['case', ['get', 'ferry'], route.ferryColor, route.color];
  const stripeColor: ExpressionSpecification = ['case', ['get', 'ferry'], route.ferryColor, route.stripeColor];
  return {
    satellite: {
      'raster-brightness-min': basemap.brightness,
      'raster-contrast': basemap.contrast,
      'raster-saturation': basemap.saturation,
    },
    hillshade: { 'hillshade-exaggeration': basemap.hillshade },
    tint: { 'background-color': basemap.tintColor, 'background-opacity': basemap.tintOpacity },
    boundary_2: border,
    boundary_3: border,
    // Bandes : deux traits de 2/3 de large décalés d'1/6 qui se chevauchent sous la bande centrale,
    // pour qu'aucun liseré de fond ne passe entre deux bandes voisines.
    traced: route.stripes
      ? {
          'line-color': routeColor,
          'line-width': byZoom((route.width * 2) / 3),
          'line-offset': byZoom(-route.width / 6),
        }
      : { 'line-color': routeColor, 'line-width': byZoom(route.width), 'line-offset': 0 },
    'traced-stripe': {
      'line-color': stripeColor,
      'line-width': byZoom((route.width * 2) / 3),
      'line-offset': byZoom(route.width / 6),
    },
    'traced-core': route.stripes
      ? { 'line-color': route.coreColor, 'line-width': byZoom(route.width / 3), 'line-blur': 0 }
      : {
          'line-color': route.coreColor,
          'line-width': byZoom(coreWidth),
          'line-blur': byZoom(coreWidth * route.coreBlurRatio),
        },
    stops: {
      'circle-color': ['case', ['get', 'visited'], route.color, stops.unvisitedColor],
      'circle-radius': byZoom(['case', ['get', 'visited'], visitedRadius, stops.unvisitedRadius]),
    },
    head: {
      'circle-color': route.headColor,
      'circle-radius': byZoom(route.headRadius),
      'circle-stroke-color': route.color,
      'circle-stroke-width': byZoom(route.width / 4),
    },
    'stops-label': {
      'text-color': theme.hud.textColor,
      'text-halo-color': theme.hud.background,
      'text-halo-width': 2,
      'text-opacity': ['get', 'labelOpacity'],
    },
    'stops-core': {
      'circle-color': route.coreColor,
      'circle-radius': byZoom(['*', route.coreRatio, visitedRadius]),
      // `circle-blur` est une fraction du rayon : même fondu relatif que le trait.
      'circle-blur': route.coreBlurRatio,
    },
  };
}

/**
 * Style complet d'un thème : du style vectoriel, seules les frontières, sur l'imagerie satellite,
 * puis le tracé et les lieux. Sans transitions : l'horloge figée par `setNow` n'en terminerait aucune,
 * et la carte ne serait jamais prête.
 */
function themedStyle(style: StyleSpecification, theme: Theme): StyleSpecification {
  const paint = paints(theme);
  // Lieux qui portent l'emoji du thème : il remplace leur point.
  const withEmoji: ExpressionSpecification | null =
    theme.stops.icon === ''
      ? null
      : theme.stops.showImageOnMap
        ? ['all', ['get', 'visited'], ['!', ['get', 'image']]]
        : ['get', 'visited'];
  const kept = style.layers.flatMap((layer): LayerSpecification[] =>
    layer.type === 'line' && KEPT_LAYERS.has(layer.id)
      ? [{ ...layer, paint: { ...layer.paint, ...paint[layer.id] } }]
      : [],
  );
  return {
    ...style,
    transition: { duration: 0, delay: 0 },
    sources: {
      ...style.sources,
      satellite: {
        type: 'raster',
        // Par le serveur de rendu (cache disque, nouvel essai) : jamais une tuile manquante à la capture.
        tiles: [`${location.origin}/imagery/${theme.basemap.imagery}/{z}/{x}/{y}`],
        tileSize: 256,
        maxzoom: IMAGERY_SOURCES[theme.basemap.imagery].maxzoom,
      },
      dem: { type: 'raster-dem', tiles: [TERRARIUM], encoding: 'terrarium', tileSize: 256, maxzoom: 12 },
      traced: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      stops: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
      head: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
    },
    layers: [
      {
        id: 'satellite',
        type: 'raster',
        source: 'satellite',
        paint: { 'raster-fade-duration': 0, ...paint['satellite'] },
      },
      {
        id: 'hillshade',
        type: 'hillshade',
        source: 'dem',
        paint: {
          'hillshade-shadow-color': '#1f2a1a',
          'hillshade-highlight-color': '#fff7e0',
          ...paint['hillshade'],
        },
      },
      { id: 'tint', type: 'background', paint: paint['tint'] },
      ...kept,
      ...(theme.route.stripes
        ? [
            {
              id: 'traced-stripe',
              type: 'line',
              source: 'traced',
              layout: ROUND,
              paint: paint['traced-stripe'],
            } satisfies LayerSpecification,
          ]
        : []),
      { id: 'traced', type: 'line', source: 'traced', layout: ROUND, paint: paint['traced'] },
      { id: 'traced-core', type: 'line', source: 'traced', layout: ROUND, paint: paint['traced-core'] },
      ...(theme.route.headRadius === 0
        ? []
        : theme.route.headIcon === ''
          ? [{ id: 'head', type: 'circle', source: 'head', paint: paint['head'] } satisfies LayerSpecification]
          : [
              {
                id: 'head',
                type: 'symbol',
                source: 'head',
                layout: {
                  'icon-image': HEAD_ICON,
                  // Diamètre : deux fois `headRadius`, comme le point qu'il remplace.
                  'icon-size': byZoom((theme.route.headRadius * 2) / ICON_PIXELS),
                  'icon-allow-overlap': true,
                  'icon-ignore-placement': true,
                },
              } satisfies LayerSpecification,
            ]),
      {
        id: 'stops',
        type: 'circle',
        source: 'stops',
        filter: withEmoji === null ? true : ['!', withEmoji],
        paint: paint['stops'],
      },
      {
        id: 'stops-core',
        type: 'circle',
        source: 'stops',
        filter: withEmoji === null ? ['get', 'visited'] : ['all', ['get', 'visited'], ['!', withEmoji]],
        paint: paint['stops-core'],
      },
      ...(theme.stops.showImageOnMap
        ? [
            {
              id: 'stops-image',
              type: 'symbol',
              source: 'stops',
              filter: ['all', ['get', 'visited'], ['get', 'image']],
              layout: {
                'icon-image': ['concat', PLACE_IMAGE_PREFIX, ['get', 'id']],
                // Taille fixe (lisible à toutes les échelles), grossie au « pop » puis revenue.
                'icon-size': ['+', 1, ['*', 0.6, ['get', 'pop']]],
                'icon-anchor': 'bottom',
                'icon-allow-overlap': true,
                'icon-ignore-placement': true,
              },
            } satisfies LayerSpecification,
          ]
        : []),
      ...(theme.stops.showLabels
        ? [
            {
              id: 'stops-label',
              type: 'symbol',
              source: 'stops',
              filter: ['get', 'visited'],
              layout: {
                'text-field': ['get', 'label'],
                // Seules polices du serveur de glyphes du fond (OpenFreeMap) : pas celle de la carte d'infos.
                'text-font': ['Noto Sans Bold'],
                'text-size': theme.stops.labelSize,
                // Sous le lieu (l'emoji est au-dessus), sinon à côté : MapLibre prend la première place libre.
                'text-variable-anchor': ['top', 'right', 'left'],
                'text-radial-offset': 0.8,
                // Le dernier lieu atteint passe avant les autres quand deux noms se chevauchent.
                'symbol-sort-key': ['-', ['get', 'index']],
              },
              paint: paint['stops-label'],
            } satisfies LayerSpecification,
          ]
        : []),
      ...(withEmoji === null
        ? []
        : [
            {
              id: 'stops-icon',
              type: 'symbol',
              source: 'stops',
              filter: withEmoji,
              layout: {
                'icon-image': STOP_ICON,
                // L'emoji grossit au « pop » puis revient à sa taille ; posé au-dessus du point du lieu.
                'icon-size': byZoom(['*', theme.stops.iconSize / ICON_PIXELS, ['+', 1, ['*', 0.8, ['get', 'pop']]]]),
                'icon-anchor': 'bottom',
                'icon-allow-overlap': true,
                'icon-ignore-placement': true,
              },
            } satisfies LayerSpecification,
          ]),
    ],
  };
}

const ROUND = { 'line-cap': 'round', 'line-join': 'round' } as const;

const STOP_ICON = 'stop-icon';
const HEAD_ICON = 'head-icon';
const PLACE_IMAGE_PREFIX = 'place-';
/** Taille de l'emoji dans l'image envoyée à MapLibre (dessinée en double résolution). */
const ICON_PIXELS = 64;

/**
 * Images des lieux dans MapLibre, à `size` pixels (dessinées en double résolution, centrées sans déformation).
 * Passent par un canvas : MapLibre ne sait pas dimensionner un SVG lui-même.
 */
function setPlaceImages(map: MapLibre, decoded: ReadonlyMap<string, HTMLImageElement>, size: number): void {
  const pixels = size * 2;
  for (const [id, image] of decoded) {
    const name = `${PLACE_IMAGE_PREFIX}${id}`;
    if (map.hasImage(name)) map.removeImage(name);
    const canvas = document.createElement('canvas');
    canvas.width = pixels;
    canvas.height = pixels;
    const context = canvas.getContext('2d');
    if (context === null) continue;
    const scale = Math.min(pixels / (image.naturalWidth || pixels), pixels / (image.naturalHeight || pixels));
    const [width, height] = [(image.naturalWidth || pixels) * scale, (image.naturalHeight || pixels) * scale];
    context.drawImage(image, (pixels - width) / 2, pixels - height, width, height);
    map.addImage(name, context.getImageData(0, 0, pixels, pixels), { pixelRatio: 2 });
  }
}

/** Dessine l'emoji du thème dans une image MapLibre, avec la police d'emojis en couleur du système. */
function setEmojiImage(map: MapLibre, name: string, icon: string): void {
  if (map.hasImage(name)) map.removeImage(name);
  if (icon === '') return;
  const size = ICON_PIXELS * 2;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext('2d');
  if (context === null) return;
  context.font = `${String(size * 0.8)}px 'Noto Color Emoji', 'Apple Color Emoji', 'Segoe UI Emoji', sans-serif`;
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  context.fillText(icon, size / 2, size / 2 + size * 0.05);
  map.addImage(name, context.getImageData(0, 0, size, size), { pixelRatio: 2 });
}

/** Emoji des lieux atteints et emoji de la tête du tracé, en images MapLibre. */
function setIcons(map: MapLibre, theme: Theme): void {
  setEmojiImage(map, STOP_ICON, theme.stops.icon);
  setEmojiImage(map, HEAD_ICON, theme.route.headIcon);
}

/** Couleurs, police et éléments affichés de la carte d'infos, en variables CSS. */
function applyHud(theme: Theme, hasImages: boolean): Promise<unknown> {
  const { hud } = theme;
  const root = document.documentElement.style;
  root.setProperty('--hud-font', `'${hud.font}'`);
  root.setProperty('--hud-size', `${String(hud.size)}px`);
  root.setProperty('--hud-bg', hud.background);
  root.setProperty('--hud-text', hud.textColor);
  root.setProperty('--hud-title', hud.titleColor);
  root.setProperty('--hud-distance', hud.distanceColor);
  root.setProperty('--pill-from', hud.pillFrom);
  root.setProperty('--pill-to', hud.pillTo);
  root.setProperty('--pill-text', hud.pillTextColor);
  element('title').classList.toggle('off', !hud.showTitle);
  element('count').classList.toggle('off', !hud.showCount);
  element('last-stop').classList.toggle('off', !hud.showLastStop);
  element('stop-image').classList.toggle('off', !hud.showImage || !hasImages);
  element('imagery-credit').textContent = IMAGERY_SOURCES[theme.basemap.imagery].credit;
  // Police chargée avant toute capture, sinon des images sortent en police de repli.
  return Promise.all([document.fonts.load(`800 1em '${hud.font}'`), document.fonts.load(`900 1em '${hud.font}'`)]);
}

function draw(
  map: MapLibre,
  tour: Tour,
  drawn: MeasuredPath,
  video: VideoSettings,
  theme: Theme,
  images: ReadonlyMap<string, string>,
  moment: Moment,
): void {
  const total = totalMeters(tour.path);
  const meters = moment.phase === 'travel' ? moment.meters : moment.phase === 'intro' ? 0 : total;
  const hud = hudAt(tour, meters);
  const popMeters = (total / video.durationSec) * POP_SECONDS;

  void source(map, 'traced').setData({
    type: 'FeatureCollection',
    features: splitCrossings(drawn, tour.ferries, meters).map((piece) => line(piece.points, { ferry: piece.ferry })),
  });
  // Tête : seulement pendant le parcours (ni avant le départ, ni sur la vue d'ensemble finale).
  const head = pointAt(drawn, meters);
  void source(map, 'head').setData({
    type: 'FeatureCollection',
    features:
      moment.phase === 'travel'
        ? [{ type: 'Feature', properties: {}, geometry: { type: 'Point', coordinates: [head[0], head[1]] } }]
        : [],
  });
  void source(map, 'stops').setData({
    type: 'FeatureCollection',
    features: tour.stops.map((stop, index) => ({
      type: 'Feature',
      properties: {
        id: stop.id,
        index,
        // « Agen · Lot-et-Garonne (47) » → « Agen » : le reste encombre la carte, la carte d'infos l'affiche.
        label: stop.name.split(' · ')[0] ?? stop.name,
        // Effacés pendant le dézoom final : à l'échelle du pays, ils couvriraient le tracé (et la couverture).
        labelOpacity: moment.phase === 'outro' ? Math.max(0, 1 - moment.progress / LABEL_FADE_PROGRESS) : 1,
        image: images.has(stop.id),
        visited: index < hud.visited,
        pop: index < hud.visited ? Math.max(0, 1 - (meters - stop.meters) / popMeters) : 0,
      },
      geometry: { type: 'Point', coordinates: [stop.at[0], stop.at[1]] },
    })),
  });

  const camera = cameraAt(tour, moment, theme.camera.followFraction);
  map.jumpTo({
    center: [camera.center[0], camera.center[1]],
    zoom: camera.zoom,
    pitch: camera.pitch,
    bearing: camera.bearing,
  });

  const km = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 0 });
  element('distance').textContent = `${km.format(hud.meters / 1000)} km`;
  element('total-distance').textContent = `/ ${km.format(hud.totalMeters / 1000)} km`;
  element('count').textContent = `${String(hud.visited)} / ${String(hud.total)} lieux`;
  const lastStop = element('last-stop');
  lastStop.textContent = hud.lastStop ?? '';
  lastStop.style.visibility = hud.lastStop === null ? 'hidden' : '';
  fitOnOneLine(lastStop);
  const image = element('stop-image');
  const imageUrl = images.get(tour.stops[hud.visited - 1]?.id ?? '') ?? null;
  if (image instanceof HTMLImageElement && image.getAttribute('src') !== imageUrl) {
    if (imageUrl === null) image.removeAttribute('src');
    else image.src = imageUrl;
  }
  image.style.visibility = imageUrl === null ? 'hidden' : '';
  document.body.dataset['phase'] = moment.phase;
}

/** Images décodées avant la première capture : sinon une image tombe sur la vidéo une fois sur deux. */
async function preload(images: ReadonlyMap<string, string>): Promise<Map<string, HTMLImageElement>> {
  const decoded = await Promise.all(
    [...images].map(async ([id, url]) => {
      const image = new Image();
      image.src = url;
      return image.decode().then(
        () => [[id, image] as const],
        () => [],
      );
    }),
  );
  return new Map(decoded.flat());
}

/** Réduit la police d'un nom trop long jusqu'à ce qu'il tienne sur sa ligne (au-delà, points de suspension). */
function fitOnOneLine(element: HTMLElement): void {
  element.style.fontSize = '';
  for (let scale = 0.95; element.scrollWidth > element.clientWidth && scale >= MIN_NAME_SCALE; scale -= 0.05) {
    element.style.fontSize = `${String(scale)}em`;
  }
}

/** Intervalle des rendus redemandés pendant l'attente des tuiles. */
const REPAINT_MS = 250;

/**
 * Résolue après le premier rendu où toutes les tuiles sont chargées. Pas `idle` : avec l'horloge
 * figée, MapLibre peut redemander des images indéfiniment et `idle` ne vient jamais. Rendu redemandé
 * régulièrement : une dernière tuile arrivée sans nouveau rendu laissait la capture attendre pour toujours.
 */
function fullyRendered(map: MapLibre): Promise<void> {
  return new Promise((resolve) => {
    const check = (): void => {
      if (!map.loaded() || !map.areTilesLoaded()) return;
      map.off('render', check);
      clearInterval(repaint);
      resolve();
    };
    const repaint = setInterval(() => {
      map.triggerRepaint();
    }, REPAINT_MS);
    map.on('render', check);
    map.triggerRepaint();
  });
}

/** Sources de la carte encore en chargement : diagnostic d'une image qui ne se termine pas. */
function pendingSources(map: MapLibre): string[] {
  return Object.keys(map.getStyle().sources).filter((id) => !map.isSourceLoaded(id));
}

/** Charge `/run.json`, `/tour.json` et `/theme.json`, crée la carte, expose `renderFrame` et `applyTheme`. */
export async function start(): Promise<void> {
  const [rawRun, rawTour, rawTheme, rawImages] = await Promise.all([
    fetchJson('/run.json'),
    fetchJson('/tour.json'),
    fetchJson('/theme.json'),
    fetchJson('/images.json'),
  ]);
  const images = new Map(
    isRecord(rawImages)
      ? Object.entries(rawImages).flatMap(([id, url]) => (typeof url === 'string' ? [[id, url] as const] : []))
      : [],
  );
  const run = parseRun(rawRun);
  if (!run.ok) throw new PageLoadError('run.json');
  const parsedTheme = parseTheme(rawTheme);
  if (!parsedTheme.ok) throw new PageLoadError('theme.json');
  let theme = parsedTheme.value;
  const tour = parseTour(rawTour);
  // Tracé allégé pour le dessin seulement : renvoyer 1 M de points au worker à chaque image coûte trop cher.
  const drawn = decimate(tour.path, totalMeters(tour.path) / MAX_DRAWN_POINTS);
  const { video } = run.value;
  const frames = timeline(video);

  const map = new MapLibre({
    container: 'map',
    interactive: false,
    attributionControl: false,
    fadeDuration: 0,
    canvasContextAttributes: { preserveDrawingBuffer: true, antialias: true },
  });
  // Style vectoriel d'origine gardé pour reconstruire le style complet à chaque changement de thème.
  let base: StyleSpecification | null = null;
  map.setStyle(BASEMAP, {
    transformStyle: (_, next) => {
      base = next;
      return themedStyle(next, theme);
    },
  });
  const [, , decoded] = await Promise.all([map.once('load'), applyHud(theme, images.size > 0), preload(images)]);
  setIcons(map, theme);
  setPlaceImages(map, decoded, theme.stops.imageSize);
  element('title').textContent = runTitle(rawRun) ?? `${String(tour.stops.length)} lieux · ${run.value.name}`;

  window.renderFrame = async (frame: number): Promise<void> => {
    // Horloge MapLibre figée sur l'image : rendu identique quelle que soit la vitesse de capture.
    setNow((frame * 1000) / video.fps);
    draw(map, tour, drawn, video, theme, images, momentAt(frames, tour, frame, theme.camera.stopSlowdown));
    const stopImage = element('stop-image');
    // L'image du lieu doit être affichée avant la capture, comme les tuiles de la carte.
    const decoded = stopImage instanceof HTMLImageElement && stopImage.src !== '' ? stopImage.decode() : null;
    await Promise.all([decoded?.catch(() => undefined), fullyRendered(map)]);
  };
  window.applyTheme = async (raw: unknown): Promise<readonly ThemeIssue[]> => {
    const next = parseTheme(raw);
    if (!next.ok) return next.errors;
    theme = next.value;
    // `diff` : MapLibre n'applique que les propriétés changées, sans recharger tuiles ni sources.
    setIcons(map, theme);
    setPlaceImages(map, decoded, theme.stops.imageSize);
    if (base !== null) map.setStyle(themedStyle(base, theme), { diff: true });
    await applyHud(theme, images.size > 0);
    return [];
  };
  window.pendingSources = () => pendingSources(map);
  window.frameCount = frameCount(frames);

  const requested = new URLSearchParams(location.search).get('frame');
  if (requested !== null && Number.isInteger(Number(requested))) await window.renderFrame(Number(requested));
}

void start();
