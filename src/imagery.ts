// Partagé par la page (zoom max, crédit) et le serveur de rendu (adresse des tuiles) : aucun import Node ici.
import type { Imagery } from './theme.ts';

/** Fonds satellite : adresse des tuiles chez le fournisseur (`{z}`, `{x}`, `{y}`), zoom max, crédit à afficher. */
export const IMAGERY_SOURCES = {
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
  /**
   * Sentinel-2 cloudless 2016 d'EOX, 10 m/pixel, mer comprise. Le millésime 2016 seulement : il est en
   * CC BY 4.0, les suivants en CC BY-NC-SA (pas d'usage commercial).
   */
  sentinel: {
    tiles: 'https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/{z}/{y}/{x}.jpg',
    maxzoom: 14,
    credit: 'Sentinel-2 cloudless by EOX (Copernicus Sentinel data 2016)',
  },
} as const satisfies Record<Imagery, { tiles: string; maxzoom: number; credit: string }>;
