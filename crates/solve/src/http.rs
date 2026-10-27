//! Adaptateur HTTP du port `Routing`.

use dijkstravel_core::geo::LonLat;
use serde_json::Value;

use crate::osrm::{self, DistanceTable, RoutedPath};
use crate::{Routing, SolveError};

/// OSRM et VROOM en HTTP.
pub struct Services {
    pub osrm_url: String,
    pub vroom_url: String,
    agent: ureq::Agent,
}

impl Services {
    pub fn new(osrm_url: impl Into<String>, vroom_url: impl Into<String>) -> Self {
        // OSRM répond 400 avec un corps JSON qui dit pourquoi (NoRoute…) : le corps est lu quel que soit le statut.
        let agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        Services { osrm_url: osrm_url.into(), vroom_url: vroom_url.into(), agent }
    }

    fn get_json(&self, url: &str) -> Result<(u16, Option<Value>), String> {
        let mut response = self.agent.get(url).call().map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        Ok((status, response.body_mut().with_config().limit(1 << 30).read_json().ok()))
    }
}

impl Routing for Services {
    fn table(&self, locations: &[LonLat]) -> Result<DistanceTable, SolveError> {
        let (status, raw) = self.get_json(&osrm::table_url(&self.osrm_url, locations)).map_err(SolveError::Osrm)?;
        osrm::parse_table(&osrm::checked(status, raw)?, locations.len())
    }

    fn route(&self, waypoints: &[LonLat]) -> Result<RoutedPath, SolveError> {
        let (status, raw) = self.get_json(&osrm::route_url(&self.osrm_url, waypoints)).map_err(SolveError::Osrm)?;
        osrm::parse_route(&osrm::checked(status, raw)?)
    }

    fn optimize(&self, request: &Value) -> Result<Value, SolveError> {
        let mut response = self
            .agent
            .post(format!("{}/", self.vroom_url))
            .send_json(request)
            .map_err(|e| SolveError::Vroom(e.to_string()))?;
        let status = response.status().as_u16();
        response
            .body_mut()
            .with_config()
            .limit(1 << 30)
            .read_json()
            .map_err(|_| SolveError::Vroom(format!("HTTP {status} sans corps JSON")))
    }
}
