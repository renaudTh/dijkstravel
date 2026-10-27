//! Sources réutilisables de lieux, chacune vers `places.geojson`. Une source ponctuelle reste un script
//! jetable qui produit un CSV ou un GeoJSON : il passe par `csv`, puis le résultat est versionné avec sa
//! provenance dans `run.json`.

pub mod atp;
pub mod csv;
pub mod overpass;
