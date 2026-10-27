//! Toute la configuration vient de l'environnement, lue et vérifiée une fois au démarrage.

use std::path::PathBuf;

use anyhow::{Context, bail};

use crate::application::Settings;
use crate::infrastructure::garage::GarageConfig;
use crate::infrastructure::routing::RoutingConfig;
use crate::infrastructure::s3::S3Config;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub s3: S3Config,
    /// Absent : Garage n'est pas initialisé par le studio (S3 déjà prêt).
    pub garage: Option<GarageConfig>,
    pub routing: RoutingConfig,
    pub settings: Settings,
    pub listen: String,
    /// Le build Angular servi à la racine.
    pub web_dir: PathBuf,
    /// Un dépôt dijkstravel à importer au démarrage de l'API, et ses sorties.
    pub import: Option<(PathBuf, PathBuf)>,
    pub worker_name: String,
    pub worker_concurrency: usize,
}

type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn optional(env: Env, name: &str) -> Option<String> {
    env(name).filter(|value| !value.trim().is_empty())
}

fn required(env: Env, name: &str) -> anyhow::Result<String> {
    optional(env, name).with_context(|| format!("variable d'environnement {name} attendue"))
}

fn or(env: Env, name: &str, fallback: &str) -> String {
    optional(env, name).unwrap_or_else(|| fallback.to_owned())
}

fn number<T: std::str::FromStr>(env: Env, name: &str, fallback: T) -> anyhow::Result<T> {
    match optional(env, name) {
        None => Ok(fallback),
        Some(value) => value.parse().map_err(|_| anyhow::anyhow!("{name} : nombre attendu, pas {value}")),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Config> {
        Config::from(&|name| std::env::var(name).ok())
    }

    pub fn from(env: Env) -> anyhow::Result<Config> {
        let s3 = S3Config {
            endpoint: or(env, "S3_ENDPOINT", "http://garage:3900"),
            region: or(env, "S3_REGION", "garage"),
            bucket: or(env, "S3_BUCKET", "dijkstravel"),
            access_key_id: required(env, "S3_ACCESS_KEY_ID")?,
            secret_access_key: required(env, "S3_SECRET_ACCESS_KEY")?,
        };
        let garage = optional(env, "GARAGE_ADMIN_TOKEN")
            .map(|admin_token| -> anyhow::Result<GarageConfig> {
                Ok(GarageConfig {
                    admin_url: or(env, "GARAGE_ADMIN_URL", "http://garage:3903"),
                    admin_token,
                    bucket: s3.bucket.clone(),
                    access_key_id: s3.access_key_id.clone(),
                    secret_access_key: s3.secret_access_key.clone(),
                    capacity: number(env, "GARAGE_CAPACITY_BYTES", 200_000_000_000)?,
                })
            })
            .transpose()?;
        let render_threads = match number(env, "RENDER_THREADS", 0usize)? {
            0 => std::thread::available_parallelism().map_or(4, std::num::NonZero::get),
            threads => threads,
        };
        let worker_concurrency = number(env, "WORKER_CONCURRENCY", 1usize)?;
        if worker_concurrency == 0 {
            bail!("WORKER_CONCURRENCY : au moins 1");
        }
        let import = optional(env, "IMPORT_DIR").map(PathBuf::from).map(|repo| {
            let out = optional(env, "IMPORT_OUT_DIR").map_or_else(|| repo.join("data/out"), PathBuf::from);
            (repo, out)
        });
        Ok(Config {
            database_url: required(env, "DATABASE_URL")?,
            s3,
            garage,
            routing: RoutingConfig {
                car: optional(env, "OSRM_URL"),
                foot: optional(env, "OSRM_FOOT_URL"),
                train: optional(env, "OSRM_TRAIN_URL"),
                vroom: or(env, "VROOM_URL", "http://vroom:3000"),
            },
            settings: Settings {
                cache_dir: PathBuf::from(or(env, "CACHE_DIR", "/cache")),
                scratch_dir: PathBuf::from(or(env, "SCRATCH_DIR", "/work")),
                render_threads,
                overpass_url: or(env, "OVERPASS_URL", dijkstravel_sources::overpass::OVERPASS_URL),
            },
            listen: or(env, "LISTEN_ADDR", "0.0.0.0:8080"),
            web_dir: PathBuf::from(or(env, "WEB_DIR", "/app/web")),
            import,
            worker_name: or(env, "WORKER_NAME", &format!("worker-{}", std::process::id())),
            worker_concurrency,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(pairs: &[(&str, &str)]) -> anyhow::Result<Config> {
        let vars: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from(&move |name| vars.get(name).cloned())
    }

    const BASE: [(&str, &str); 3] =
        [("DATABASE_URL", "postgres://x"), ("S3_ACCESS_KEY_ID", "GK1"), ("S3_SECRET_ACCESS_KEY", "s")];

    #[test]
    fn needs_the_database_and_the_s3_key() {
        assert!(config(&BASE[..2]).is_err());
        let config = config(&BASE).unwrap();
        assert_eq!(config.s3.endpoint, "http://garage:3900");
        assert!(config.garage.is_none());
        assert_eq!(config.routing.car, None);
        assert_eq!(config.worker_concurrency, 1);
    }

    #[test]
    fn reads_optional_services_and_numbers() {
        let mut pairs = BASE.to_vec();
        pairs.extend([
            ("GARAGE_ADMIN_TOKEN", "t"),
            ("OSRM_URL", "http://osrm-car:5000"),
            ("OSRM_TRAIN_URL", " "),
            ("RENDER_THREADS", "3"),
            ("IMPORT_DIR", "/seed"),
        ]);
        let config = config(&pairs).unwrap();
        assert_eq!(config.garage.unwrap().bucket, "dijkstravel");
        assert_eq!(config.routing.car.as_deref(), Some("http://osrm-car:5000"));
        assert_eq!(config.routing.train, None);
        assert_eq!(config.settings.render_threads, 3);
        assert_eq!(config.import, Some((PathBuf::from("/seed"), PathBuf::from("/seed/data/out"))));
    }

    #[test]
    fn rejects_a_malformed_number() {
        let mut pairs = BASE.to_vec();
        pairs.push(("WORKER_CONCURRENCY", "deux"));
        assert!(config(&pairs).is_err());
    }
}
