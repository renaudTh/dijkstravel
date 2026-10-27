//! Mise en route de Garage par son API d'admin (v2), à chaque démarrage et sans effet si déjà faite :
//! layout à un nœud, clé d'accès du studio, bucket et droits de la clé sur le bucket.

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct GarageConfig {
    /// `http://garage:3903`.
    pub admin_url: String,
    pub admin_token: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Capacité annoncée du nœud, en octets.
    pub capacity: u64,
}

struct Admin<'a> {
    config: &'a GarageConfig,
    agent: ureq::Agent,
}

impl Admin<'_> {
    /// Statut HTTP et corps JSON (s'il y en a un).
    fn call(&self, method: &str, endpoint: &str, body: Option<Value>) -> anyhow::Result<(u16, Value)> {
        let url = format!("{}/v2/{endpoint}", self.config.admin_url.trim_end_matches('/'));
        let authorization = format!("Bearer {}", self.config.admin_token);
        let mut response = match (method, body) {
            ("GET", _) => self.agent.get(&url).header("authorization", &authorization).call(),
            (_, Some(body)) => self.agent.post(&url).header("authorization", &authorization).send_json(body),
            (_, None) => self.agent.post(&url).header("authorization", &authorization).send_empty(),
        }
        .with_context(|| format!("Garage : {endpoint}"))?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_json::<Value>().unwrap_or(Value::Null);
        Ok((status, body))
    }

    fn ok(&self, method: &str, endpoint: &str, body: Option<Value>) -> anyhow::Result<Value> {
        let (status, body) = self.call(method, endpoint, body)?;
        if !(200..300).contains(&status) {
            bail!("Garage : {endpoint} (HTTP {status}) {body}");
        }
        Ok(body)
    }
}

/// Bloquant : à lancer hors du runtime async.
pub fn bootstrap(config: &GarageConfig) -> anyhow::Result<()> {
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    let admin = Admin { config, agent };

    // Garage peut démarrer après le studio : on l'attend un moment.
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match admin.ok("GET", "GetClusterStatus", None) {
            Ok(status) => break status,
            Err(error) if Instant::now() < deadline => {
                tracing::info!("Garage pas encore prêt : {error:#}");
                std::thread::sleep(Duration::from_secs(2));
            }
            Err(error) => return Err(error),
        }
    };

    let node = &status["nodes"][0];
    if node["role"].is_null() {
        let id = node["id"].as_str().context("Garage : nœud sans identifiant")?;
        admin.ok(
            "POST",
            "UpdateClusterLayout",
            Some(json!({ "roles": [{ "id": id, "zone": "local", "capacity": config.capacity, "tags": [] }] })),
        )?;
        let layout = admin.ok("GET", "GetClusterLayout", None)?;
        let version = layout["version"].as_u64().unwrap_or(0) + 1;
        admin.ok("POST", "ApplyClusterLayout", Some(json!({ "version": version })))?;
        tracing::info!(node = id, "Garage : layout appliqué");
    }

    let (key_status, _) = admin.call("GET", &format!("GetKeyInfo?id={}", config.access_key_id), None)?;
    if !(200..300).contains(&key_status) {
        admin.ok(
            "POST",
            "ImportKey",
            Some(json!({
                "accessKeyId": config.access_key_id,
                "secretAccessKey": config.secret_access_key,
                "name": "dijkstravel-studio",
            })),
        )?;
        tracing::info!("Garage : clé importée");
    }

    let (bucket_status, bucket) = admin.call("GET", &format!("GetBucketInfo?globalAlias={}", config.bucket), None)?;
    let bucket = if (200..300).contains(&bucket_status) {
        bucket
    } else {
        tracing::info!(bucket = config.bucket, "Garage : bucket créé");
        admin.ok("POST", "CreateBucket", Some(json!({ "globalAlias": config.bucket })))?
    };
    let bucket_id = bucket["id"].as_str().context("Garage : bucket sans identifiant")?;
    admin.ok(
        "POST",
        "AllowBucketKey",
        Some(json!({
            "bucketId": bucket_id,
            "accessKeyId": config.access_key_id,
            "permissions": { "read": true, "write": true, "owner": true },
        })),
    )?;
    Ok(())
}
