//! Contexte Planification : la tournée la plus courte. Le domaine (`solve`) ne connaît que le port
//! `Routing` ; l'adaptateur `http::Services` le sert par OSRM (distances, tracés) et VROOM (ordre de
//! visite), dont `osrm` et `vroom` traduisent les formats. VROOM ne reçoit que des matrices : il
//! n'appelle jamais OSRM.

pub mod http;
pub mod osrm;
pub mod solve;
pub mod vroom;

use dijkstravel_core::geo::LonLat;
use serde_json::Value;

pub use crate::http::Services;
pub use crate::osrm::{DistanceTable, RoutedPath};
pub use crate::solve::{SolveError, solve_tour};

/// Les services de routage, derrière une interface : les tests en donnent un faux.
pub trait Routing {
    fn table(&self, locations: &[LonLat]) -> Result<DistanceTable, SolveError>;
    fn route(&self, waypoints: &[LonLat]) -> Result<RoutedPath, SolveError>;
    /// Réponse brute de VROOM à `request`.
    fn optimize(&self, request: &Value) -> Result<Value, SolveError>;
}
