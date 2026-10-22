// Côté navigateur. `?frame=<n>` affiche l'image n sans capture (CA-43).
import {
  Map as MapLibre,
  setNow,
  type ExpressionSpecification,
  type GeoJSONSource,
  type LayerSpecification,
  type StyleSpecification,
} from 'maplibre-gl';
import { decimate, pathUntil, totalMeters, type MeasuredPath } from '../geo.ts';
import { isRecord } from '../narrow.ts';
import { parseRun, type VideoSettings } from '../run.ts';
import { parseTour, type Tour } from '../solve/tour.ts';
import { parseTheme, type Imagery, type Theme, type ThemeIssue } from '../theme.ts';
import { cameraAt, frameCount, hudAt, momentAt, timeline, type Moment } from './frame.ts';

declare global {
  interface Window {
    /** Résolue quand la carte est entièrement chargée (`idle`). */
    renderFrame: (frame: number) => Promise<void>;
    frameCount: number;
    /** Applique un thème à chaud (éditeur) ; renvoie ses problèmes, sans rien changer s'il en a. */
    applyTheme: (raw: unknown) => Promise<readonly ThemeIssue[]>;
  }
}

const BASEMAP = 'https://tiles.openfreemap.org/styles/liberty';
const IMAGERY_SOURCES = {
  /** NASA Blue Marble (domaine public), 500 m/pixel : volontairement peu détaillé, rien au-delà du zoom 8. */
  nasa: {
    tiles:
      'https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/BlueMarble_NextGeneration/default/GoogleMapsCompatible_Level8/{z}/{y}/{x}.jpeg',
    maxzoom: 8,
    credit: 'NASA Blue Marble',
  },
  /** Orthophoto IGN (licence ouverte Etalab 2.0), 20 cm/pixel, France seulement. */
  ign: {
    tiles:
      'https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=ORTHOIMAGERY.ORTHOPHOTOS&STYLE=normal&TILEMATRIXSET=PM&FORMAT=image/jpeg&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}',
    maxzoom: 19,
    credit: 'Orthophoto © IGN',
  },
} as const satisfies Record<Imagery, { tiles: string; maxzoom: number; credit: string }>;
const KEPT_LAYERS = new Set(['boundary_2', 'boundary_3']);
const TERRARIUM = 'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png';
/** Durée de l'effet « pop » d'un lieu atteint, en secondes de vidéo. */
const POP_SECONDS = 0.6;
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

const line = (coordinates: readonly (readonly [number, number])[]): GeoJSON.Feature => ({
  type: 'Feature',
  properties: {},
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
    traced: { 'line-color': route.color, 'line-width': byZoom(route.width) },
    'traced-core': {
      'line-color': route.coreColor,
      'line-width': byZoom(coreWidth),
      'line-blur': byZoom(coreWidth * route.coreBlurRatio),
    },
    stops: {
      'circle-color': ['case', ['get', 'visited'], route.color, stops.unvisitedColor],
      'circle-radius': byZoom(['case', ['get', 'visited'], visitedRadius, stops.unvisitedRadius]),
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
        tiles: [IMAGERY_SOURCES[theme.basemap.imagery].tiles],
        tileSize: 256,
        maxzoom: IMAGERY_SOURCES[theme.basemap.imagery].maxzoom,
      },
      dem: { type: 'raster-dem', tiles: [TERRARIUM], encoding: 'terrarium', tileSize: 256, maxzoom: 12 },
      traced: { type: 'geojson', data: line([]) },
      stops: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
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
      { id: 'traced', type: 'line', source: 'traced', layout: ROUND, paint: paint['traced'] },
      { id: 'traced-core', type: 'line', source: 'traced', layout: ROUND, paint: paint['traced-core'] },
      { id: 'stops', type: 'circle', source: 'stops', paint: paint['stops'] },
      { id: 'stops-core', type: 'circle', source: 'stops', filter: ['get', 'visited'], paint: paint['stops-core'] },
    ],
  };
}

const ROUND = { 'line-cap': 'round', 'line-join': 'round' } as const;

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

  void source(map, 'traced').setData(line(pathUntil(drawn, meters)));
  void source(map, 'stops').setData({
    type: 'FeatureCollection',
    features: tour.stops.map((stop, index) => ({
      type: 'Feature',
      properties: {
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
function preload(images: ReadonlyMap<string, string>): Promise<unknown> {
  return Promise.all(
    [...images.values()].map((url) => {
      const image = new Image();
      image.src = url;
      return image.decode().catch(() => undefined);
    }),
  );
}

/** Réduit la police d'un nom trop long jusqu'à ce qu'il tienne sur sa ligne (au-delà, points de suspension). */
function fitOnOneLine(element: HTMLElement): void {
  element.style.fontSize = '';
  for (let scale = 0.95; element.scrollWidth > element.clientWidth && scale >= MIN_NAME_SCALE; scale -= 0.05) {
    element.style.fontSize = `${String(scale)}em`;
  }
}

/**
 * Résolue après le premier rendu où toutes les tuiles sont chargées. Pas `idle` : avec l'horloge
 * figée, MapLibre peut redemander des images indéfiniment et `idle` ne vient jamais.
 */
function fullyRendered(map: MapLibre): Promise<void> {
  return new Promise((resolve) => {
    const check = (): void => {
      if (!map.loaded() || !map.areTilesLoaded()) return;
      map.off('render', check);
      resolve();
    };
    map.on('render', check);
    map.triggerRepaint();
  });
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
  await Promise.all([map.once('load'), applyHud(theme, images.size > 0), preload(images)]);
  element('title').textContent = `${String(tour.stops.length)} lieux · ${run.value.name}`;

  window.renderFrame = async (frame: number): Promise<void> => {
    // Horloge MapLibre figée sur l'image : rendu identique quelle que soit la vitesse de capture.
    setNow((frame * 1000) / video.fps);
    draw(map, tour, drawn, video, theme, images, momentAt(frames, tour, frame));
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
    if (base !== null) map.setStyle(themedStyle(base, theme), { diff: true });
    await applyHud(theme, images.size > 0);
    return [];
  };
  window.frameCount = frameCount(frames);

  const requested = new URLSearchParams(location.search).get('frame');
  if (requested !== null && Number.isInteger(Number(requested))) await window.renderFrame(Number(requested));
}

void start();
