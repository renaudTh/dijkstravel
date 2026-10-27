//! Le répertoire des routages : un OSRM par profil, un VROOM pour tous (adaptateur `solve::Services`).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dijkstravel_core::run::Profile;
use dijkstravel_solve::{Routing, Services};

use crate::domain::ports::RoutingDirectory;

#[derive(Debug, Clone)]
pub struct RoutingConfig {
    pub car: Option<String>,
    pub foot: Option<String>,
    pub train: Option<String>,
    pub vroom: String,
}

impl RoutingConfig {
    fn osrm(&self, profile: Profile) -> Option<&str> {
        match profile {
            Profile::Car => self.car.as_deref(),
            Profile::Foot => self.foot.as_deref(),
            Profile::Train => self.train.as_deref(),
        }
    }
}

pub struct HttpRouting {
    config: RoutingConfig,
}

impl HttpRouting {
    pub fn new(config: RoutingConfig) -> Self {
        HttpRouting { config }
    }
}

/// N'importe quelle réponse HTTP, même une erreur, montre que le service écoute.
fn answers(url: &str) -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .into();
    agent.get(url).call().is_ok()
}

#[async_trait]
impl RoutingDirectory for HttpRouting {
    fn routing(&self, profile: Profile) -> Option<Arc<dyn Routing + Send + Sync>> {
        let osrm = self.config.osrm(profile)?;
        Some(Arc::new(Services::new(osrm, &self.config.vroom)))
    }

    async fn available(&self, profile: Profile) -> bool {
        let Some(osrm) = self.config.osrm(profile) else { return false };
        let (osrm, vroom) = (format!("{osrm}/nearest/v1/driving/0,0"), format!("{}/", self.config.vroom));
        tokio::task::spawn_blocking(move || answers(&osrm) && answers(&vroom)).await.unwrap_or(false)
    }
}
