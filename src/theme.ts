// Chargé aussi par le navigateur (aperçu et rendu) : aucun import Node ici.
import { isFiniteNumber, isRecord } from './narrow.ts';
import type { Result } from './result.ts';

export type Theme = {
  readonly route: {
    readonly color: string;
    readonly coreColor: string;
    /** Largeur en pixels au zoom d'une région ; réduite au quart au zoom d'un pays. */
    readonly width: number;
    /** Largeur du cœur rapportée à celle du trait, reprise pour les cercles des lieux atteints. */
    readonly coreRatio: number;
    /** Flou du cœur rapporté à sa largeur. */
    readonly coreBlurRatio: number;
  };
  readonly stops: {
    readonly unvisitedColor: string;
    readonly unvisitedRadius: number;
    /** Rayon d'un lieu atteint, rapporté à la largeur du trait. */
    readonly visitedRatio: number;
    /** Rayon ajouté au moment où le lieu est atteint (« pop »). */
    readonly popRadius: number;
    /** Emoji posé sur chaque lieu atteint (🏟️, 🍺…), qui « pope » avec lui ; vide : aucun. */
    readonly icon: string;
    /** Taille de l'emoji en pixels, au zoom d'une région. */
    readonly iconSize: number;
    /** Image du lieu (blason…) posée sur la carte à la place de l'emoji, quand le run en fournit. */
    readonly showImageOnMap: boolean;
    /** Taille de cette image en pixels, fixe : elle doit rester lisible à toutes les échelles. */
    readonly imageSize: number;
  };
  readonly basemap: {
    readonly imagery: Imagery;
    readonly brightness: number;
    readonly contrast: number;
    readonly saturation: number;
    readonly hillshade: number;
    readonly borders: boolean;
    readonly borderColor: string;
    /** Voile de couleur posé sur l'imagerie : une ambiance (nuit, sépia…) sans toucher au tracé. */
    readonly tintColor: string;
    readonly tintOpacity: number;
  };
  readonly hud: {
    readonly font: HudFont;
    /** Taille de base du texte de la carte d'infos, en pixels. */
    readonly size: number;
    readonly background: string;
    readonly textColor: string;
    readonly titleColor: string;
    readonly distanceColor: string;
    readonly pillFrom: string;
    readonly pillTo: string;
    readonly pillTextColor: string;
    readonly showTitle: boolean;
    readonly showCount: boolean;
    readonly showLastStop: boolean;
    /** Image du dernier lieu (blason, logo…) quand le run en fournit dans `data/<run>/images/`. */
    readonly showImage: boolean;
  };
  readonly camera: {
    /** Part de l'emprise totale visible pendant le parcours. */
    readonly followFraction: number;
  };
};

/**
 * Imagerie du fond : `nasa` (Blue Marble, 500 m/pixel, pays et régions), `ign` (orthophoto IGN,
 * 20 cm/pixel, France seulement, pour une ville).
 */
export const IMAGERIES = ['nasa', 'ign'] as const;

export type Imagery = (typeof IMAGERIES)[number];

/** Polices arrondies de Google Fonts proposées dans l'éditeur. */
export const HUD_FONTS = ['Nunito', 'Quicksand', 'Fredoka', 'Baloo 2', 'Poppins', 'Comfortaa'] as const;

export type HudFont = (typeof HUD_FONTS)[number];

export const DEFAULT_THEME = {
  route: { color: '#702963', coreColor: '#e0679f', width: 16, coreRatio: 7 / 16, coreBlurRatio: 4 / 7 },
  stops: {
    unvisitedColor: '#d9d9d9',
    unvisitedRadius: 4,
    visitedRatio: 0.75,
    popRadius: 12,
    icon: '',
    iconSize: 48,
    showImageOnMap: false,
    imageSize: 70,
  },
  basemap: {
    imagery: 'nasa',
    brightness: 0.06,
    contrast: 0.15,
    saturation: 0.15,
    hillshade: 0.4,
    borders: true,
    borderColor: '#ffffff',
    tintColor: '#0b1630',
    tintOpacity: 0,
  },
  hud: {
    font: 'Nunito',
    size: 30,
    background: '#260c28',
    textColor: '#ffffff',
    titleColor: '#f5a3c7',
    distanceColor: '#e0679f',
    pillFrom: '#702963',
    pillTo: '#e0679f',
    pillTextColor: '#ffffff',
    showTitle: true,
    showCount: true,
    showLastStop: true,
    showImage: true,
  },
  camera: { followFraction: 1 / 8 },
} as const satisfies Theme;

export type ThemeIssue = { readonly path: string; readonly expected: string };

/** Bornes des réglages numériques, partagées par la validation et les curseurs de l'éditeur. */
export const NUMBER_RANGES = {
  'route.width': [2, 40],
  'route.coreRatio': [0, 1],
  'route.coreBlurRatio': [0, 1],
  'stops.unvisitedRadius': [0, 20],
  'stops.visitedRatio': [0.3, 2],
  'stops.popRadius': [0, 40],
  'stops.iconSize': [16, 160],
  'stops.imageSize': [24, 200],
  'basemap.brightness': [0, 1],
  'basemap.contrast': [-1, 1],
  'basemap.saturation': [-1, 1],
  'basemap.hillshade': [0, 1],
  'basemap.tintOpacity': [0, 0.9],
  'hud.size': [12, 60],
  'camera.followFraction': [0.02, 1],
} as const satisfies Record<string, readonly [number, number]>;

const COLOR = /^#[0-9a-f]{6}$/i;

type NumberPath = keyof typeof NUMBER_RANGES;

/**
 * Complète un thème partiel avec `DEFAULT_THEME` : un fichier de thème ne contient que ce qui change,
 * et un réglage ajouté plus tard prend sa valeur par défaut dans les anciens thèmes.
 */
export function parseTheme(raw: unknown): Result<Theme, ThemeIssue> {
  if (!isRecord(raw)) return { ok: false, errors: [{ path: '', expected: 'objet' }] };
  const issues: ThemeIssue[] = [];
  const given = (path: string): unknown => {
    const [section = '', key = ''] = path.split('.');
    const values = raw[section];
    return isRecord(values) ? values[key] : undefined;
  };
  const read = <T>(path: string, fallback: T, valid: (value: unknown) => value is T, expected: string): T => {
    const value = given(path);
    if (value === undefined) return fallback;
    if (valid(value)) return value;
    issues.push({ path, expected });
    return fallback;
  };
  const color = (path: string, fallback: string): string =>
    read(path, fallback, (value): value is string => typeof value === 'string' && COLOR.test(value), 'couleur #rrggbb');
  const flag = (path: string, fallback: boolean): boolean =>
    read(path, fallback, (value): value is boolean => typeof value === 'boolean', 'booléen');
  const number = (path: NumberPath, fallback: number): number => {
    const [min, max] = NUMBER_RANGES[path];
    return read(
      path,
      fallback,
      (value): value is number => isFiniteNumber(value) && value >= min && value <= max,
      `nombre entre ${String(min)} et ${String(max)}`,
    );
  };
  const font = read(
    'hud.font',
    DEFAULT_THEME.hud.font,
    (value): value is HudFont => HUD_FONTS.some((name) => name === value),
    HUD_FONTS.join(' | '),
  );
  const imagery = read(
    'basemap.imagery',
    DEFAULT_THEME.basemap.imagery,
    (value): value is Imagery => IMAGERIES.some((name) => name === value),
    IMAGERIES.join(' | '),
  );
  const d = DEFAULT_THEME;
  const theme: Theme = {
    route: {
      color: color('route.color', d.route.color),
      coreColor: color('route.coreColor', d.route.coreColor),
      width: number('route.width', d.route.width),
      coreRatio: number('route.coreRatio', d.route.coreRatio),
      coreBlurRatio: number('route.coreBlurRatio', d.route.coreBlurRatio),
    },
    stops: {
      unvisitedColor: color('stops.unvisitedColor', d.stops.unvisitedColor),
      unvisitedRadius: number('stops.unvisitedRadius', d.stops.unvisitedRadius),
      visitedRatio: number('stops.visitedRatio', d.stops.visitedRatio),
      popRadius: number('stops.popRadius', d.stops.popRadius),
      icon: read(
        'stops.icon',
        d.stops.icon,
        // Un emoji ou deux, pas un texte : il est dessiné en image sur la carte.
        (value): value is string => typeof value === 'string' && value.length <= 16,
        'emoji (16 unités UTF-16 au plus) ou vide',
      ),
      iconSize: number('stops.iconSize', d.stops.iconSize),
      showImageOnMap: flag('stops.showImageOnMap', d.stops.showImageOnMap),
      imageSize: number('stops.imageSize', d.stops.imageSize),
    },
    basemap: {
      imagery,
      brightness: number('basemap.brightness', d.basemap.brightness),
      contrast: number('basemap.contrast', d.basemap.contrast),
      saturation: number('basemap.saturation', d.basemap.saturation),
      hillshade: number('basemap.hillshade', d.basemap.hillshade),
      borders: flag('basemap.borders', d.basemap.borders),
      borderColor: color('basemap.borderColor', d.basemap.borderColor),
      tintColor: color('basemap.tintColor', d.basemap.tintColor),
      tintOpacity: number('basemap.tintOpacity', d.basemap.tintOpacity),
    },
    hud: {
      font,
      size: number('hud.size', d.hud.size),
      background: color('hud.background', d.hud.background),
      textColor: color('hud.textColor', d.hud.textColor),
      titleColor: color('hud.titleColor', d.hud.titleColor),
      distanceColor: color('hud.distanceColor', d.hud.distanceColor),
      pillFrom: color('hud.pillFrom', d.hud.pillFrom),
      pillTo: color('hud.pillTo', d.hud.pillTo),
      pillTextColor: color('hud.pillTextColor', d.hud.pillTextColor),
      showTitle: flag('hud.showTitle', d.hud.showTitle),
      showCount: flag('hud.showCount', d.hud.showCount),
      showLastStop: flag('hud.showLastStop', d.hud.showLastStop),
      showImage: flag('hud.showImage', d.hud.showImage),
    },
    camera: { followFraction: number('camera.followFraction', d.camera.followFraction) },
  };
  return issues.length > 0 ? { ok: false, errors: issues } : { ok: true, value: theme };
}

export function isThemeName(name: string): boolean {
  return /^[\w-]+$/.test(name);
}

export function themePath(name: string): string {
  return `themes/${name}.json`;
}
