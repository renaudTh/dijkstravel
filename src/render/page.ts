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
import { parseRun, type VideoSettings } from '../run.ts';
import { parseTour, type Tour } from '../solve/tour.ts';
import { cameraAt, frameCount, hudAt, momentAt, timeline, type Moment } from './frame.ts';

declare global {
  interface Window {
    /** Résolue quand la carte est entièrement chargée (`idle`). */
    renderFrame: (frame: number) => Promise<void>;
    frameCount: number;
  }
}

const BASEMAP = 'https://tiles.openfreemap.org/styles/liberty';
/** NASA Blue Marble (domaine public), 500 m/pixel : volontairement peu détaillé, rien au-delà du zoom 8. */
const SATELLITE =
  'https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/BlueMarble_NextGeneration/default/GoogleMapsCompatible_Level8/{z}/{y}/{x}.jpeg';
const KEPT_LAYERS = new Set(['boundary_2', 'boundary_3']);
const TERRARIUM = 'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png';
/** Violet prune, chaud et sombre : tracé et lieux atteints. */
const ROUTE_COLOR = '#702963';
/** Cœur rose du trait : bords violets, centre rose, fondu par le flou. */
const ROUTE_CORE_COLOR = '#e0679f';
/** Durée de l'effet « pop » d'un lieu atteint, en secondes de vidéo. */
const POP_SECONDS = 0.6;
const MAX_DRAWN_POINTS = 20_000;

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

/**
 * Du style vectoriel, on ne garde que les frontières, posées sur l'imagerie satellite.
 * Transformé avant chargement : un `setPaintProperty` lancerait une transition que l'horloge figée
 * par `setNow` ne terminerait jamais.
 */
function naturalStyle(style: StyleSpecification): StyleSpecification {
  const kept = style.layers.flatMap((layer): LayerSpecification[] =>
    layer.type === 'line' && KEPT_LAYERS.has(layer.id)
      ? [{ ...layer, paint: { ...layer.paint, 'line-color': 'rgba(255, 255, 255, 0.6)' } }]
      : [],
  );
  return {
    ...style,
    transition: { duration: 0, delay: 0 },
    sources: {
      ...style.sources,
      satellite: { type: 'raster', tiles: [SATELLITE], tileSize: 256, maxzoom: 8 },
      dem: { type: 'raster-dem', tiles: [TERRARIUM], encoding: 'terrarium', tileSize: 256, maxzoom: 12 },
    },
    layers: [
      {
        id: 'satellite',
        type: 'raster',
        source: 'satellite',
        paint: {
          'raster-fade-duration': 0,
          // Blue Marble est sombre en vidéo : un peu de lumière et de contraste.
          'raster-brightness-min': 0.06,
          'raster-contrast': 0.15,
          'raster-saturation': 0.15,
        },
      },
      {
        id: 'hillshade',
        type: 'hillshade',
        source: 'dem',
        paint: {
          'hillshade-exaggeration': 0.4,
          'hillshade-shadow-color': '#1f2a1a',
          'hillshade-highlight-color': '#fff7e0',
        },
      },
      ...kept,
    ],
  };
}

const ROUTE_WIDTH = 16;
/** Proportions du cœur rose, reprises pour les cercles des lieux atteints. */
const CORE_RATIO = 7 / 16;
const CORE_BLUR_RATIO = 4 / 7;
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

function addLayers(map: MapLibre): void {
  map.addSource('traced', { type: 'geojson', data: line([]) });
  map.addSource('stops', { type: 'geojson', data: { type: 'FeatureCollection', features: [] } });
  map.addLayer({
    id: 'traced',
    type: 'line',
    source: 'traced',
    layout: { 'line-cap': 'round', 'line-join': 'round' },
    paint: { 'line-color': ROUTE_COLOR, 'line-width': byZoom(ROUTE_WIDTH) },
  });
  map.addLayer({
    id: 'traced-core',
    type: 'line',
    source: 'traced',
    layout: { 'line-cap': 'round', 'line-join': 'round' },
    paint: {
      'line-color': ROUTE_CORE_COLOR,
      'line-width': byZoom(ROUTE_WIDTH * CORE_RATIO),
      'line-blur': byZoom(ROUTE_WIDTH * CORE_RATIO * CORE_BLUR_RATIO),
    },
  });
  const visitedRadius: ExpressionSpecification = ['+', ROUTE_WIDTH * 0.75, ['*', 12, ['get', 'pop']]];
  map.addLayer({
    id: 'stops',
    type: 'circle',
    source: 'stops',
    paint: {
      'circle-color': ['case', ['get', 'visited'], ROUTE_COLOR, '#d9d9d9'],
      'circle-radius': byZoom(['case', ['get', 'visited'], visitedRadius, 4]),
    },
  });
  map.addLayer({
    id: 'stops-core',
    type: 'circle',
    source: 'stops',
    filter: ['get', 'visited'],
    paint: {
      'circle-color': ROUTE_CORE_COLOR,
      'circle-radius': byZoom(['*', CORE_RATIO, visitedRadius]),
      // `circle-blur` est une fraction du rayon : même fondu relatif que le trait.
      'circle-blur': CORE_BLUR_RATIO,
    },
  });
}

function draw(map: MapLibre, tour: Tour, drawn: MeasuredPath, video: VideoSettings, moment: Moment): void {
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

  const camera = cameraAt(tour, moment);
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
  element('last-stop').textContent = hud.lastStop ?? '';
  element('last-stop').hidden = hud.lastStop === null;
  document.body.dataset['phase'] = moment.phase;
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

/** Charge `/run.json` et `/tour.json`, crée la carte, expose `renderFrame`. */
export async function start(): Promise<void> {
  const [rawRun, rawTour] = await Promise.all([fetchJson('/run.json'), fetchJson('/tour.json')]);
  const run = parseRun(rawRun);
  if (!run.ok) throw new PageLoadError('run.json');
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
  map.setStyle(BASEMAP, { transformStyle: (_, next) => naturalStyle(next) });
  // Police chargée avant la première capture, sinon les premières images sortent en police de repli.
  await Promise.all([map.once('load'), document.fonts.load('800 1em Nunito'), document.fonts.load('900 1em Nunito')]);
  addLayers(map);
  element('title').textContent = `${String(tour.stops.length)} lieux · ${run.value.name}`;

  window.renderFrame = async (frame: number): Promise<void> => {
    // Horloge MapLibre figée sur l'image : rendu identique quelle que soit la vitesse de capture.
    setNow((frame * 1000) / video.fps);
    draw(map, tour, drawn, video, momentAt(frames, tour, frame));
    await fullyRendered(map);
  };
  window.frameCount = frameCount(frames);

  const requested = new URLSearchParams(location.search).get('frame');
  if (requested !== null && Number.isInteger(Number(requested))) await window.renderFrame(Number(requested));
}

void start();
