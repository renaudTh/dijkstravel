//! Contexte Rendu : d'une tournée et d'un thème, les images et la vidéo. `scene` est l'entrée du
//! contexte ; `infra` regroupe ce qui touche au disque, au réseau et à ffmpeg ; le reste est le moteur.

pub use dijkstravel_core::geo;

pub mod atlas;
pub mod borders;
pub mod cache;
pub mod color;
pub mod frame;
pub mod hillshade;
pub mod hud;
pub mod infra;
pub mod mercator;
pub mod model;
pub mod montage;
pub mod monitor;
pub mod preview;
pub mod render;
pub mod route;
pub mod scene;
pub mod sprites;
pub mod stops;

pub use infra::{download, tiles, video};
