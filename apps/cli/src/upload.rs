//! `dijkstravel upload runs/<nom>` : envoie la vidéo, la couverture et `publication.txt` du run dans
//! `Documents/dijkstravel/<nom>/` du Nextcloud décrit dans `.env` (fichiers déjà présents remplacés).

use std::path::{Path, PathBuf};

use base64::Engine;
use ureq::http::{Method, Request};

use crate::load::{self, Outcome, fail};

/// Dossier Nextcloud qui reçoit un sous-dossier par run.
const REMOTE_ROOT: [&str; 2] = ["Documents", "dijkstravel"];

struct Connection {
    dav_root: String,
    authorization: String,
    agent: ureq::Agent,
}

/// Comme `encodeURIComponent` : un nom de fichier ou de dossier devient un segment d'URL.
fn encode_segment(segment: &str) -> String {
    segment
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Seul lecteur des variables Nextcloud, chargées depuis `.env`.
fn connection() -> Option<Connection> {
    let _ = dotenvy::dotenv();
    let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let (url, user, password) = (var("NEXTCLOUD_URL")?, var("NEXTCLOUD_USER")?, var("NEXTCLOUD_APP_PASSWORD")?);
    let credentials = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
    Some(Connection {
        dav_root: format!("{}/remote.php/dav/files/{}", url.trim_end_matches('/'), encode_segment(&user)),
        authorization: format!("Basic {credentials}"),
        // MKCOL (WebDAV) n'est pas une méthode HTTP standard : ureq la refuse sans cette option.
        agent: ureq::Agent::config_builder()
            .http_status_as_error(false)
            .allow_non_standard_methods(true)
            .build()
            .into(),
    })
}

impl Connection {
    /// Crée chaque dossier du chemin, un à un : un dossier n'existe qu'une fois son parent créé ;
    /// 405 veut dire « existe déjà ».
    fn ensure_folders(&self, segments: &[&str]) -> Outcome<String> {
        let mut path = self.dav_root.clone();
        for segment in segments {
            path = format!("{path}/{}", encode_segment(segment));
            let request = Request::builder()
                .method(Method::from_bytes(b"MKCOL").expect("méthode WebDAV"))
                .uri(&path)
                .header("authorization", &self.authorization)
                .body(())?;
            let status = self.agent.run(request)?.status().as_u16();
            if status != 201 && status != 405 {
                return fail(format!("Nextcloud : création de {segment} (HTTP {status})"));
            }
        }
        Ok(path)
    }

    /// Envoi en flux : une vidéo de 200 Mo ne passe pas par la mémoire.
    fn upload(&self, folder: &str, file: &Path) -> Outcome<u64> {
        let name = file.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        let size = std::fs::metadata(file)?.len();
        let body = std::fs::File::open(file)?;
        let response = self
            .agent
            .put(format!("{folder}/{}", encode_segment(name)))
            .header("authorization", &self.authorization)
            .header("content-length", size.to_string())
            .send(body)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return fail(format!("Nextcloud : envoi de {name} (HTTP {status})"));
        }
        Ok(size)
    }
}

pub fn main(arg: &Path) -> Outcome {
    let Some(nextcloud) = connection() else {
        return fail("NEXTCLOUD_URL, NEXTCLOUD_USER et NEXTCLOUD_APP_PASSWORD attendus dans .env");
    };
    let (run, paths) = load::run(arg)?;
    if !paths.video.exists() {
        return fail(format!(
            "{} absente : rendre la vidéo d'abord (dijkstravel render {}).",
            paths.video.display(),
            arg.display()
        ));
    }
    let files: Vec<PathBuf> = [paths.video.clone(), paths.out.join("cover.jpg"), paths.out.join("publication.txt")]
        .into_iter()
        .filter(|file| file.exists())
        .collect();
    let folder = nextcloud.ensure_folders(&[REMOTE_ROOT[0], REMOTE_ROOT[1], &run.name])?;
    // Séquentiel : un fichier à la fois sur la bande passante.
    for file in files {
        let size = nextcloud.upload(&folder, &file)?;
        let name = file.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        println!("{} → {}/{}/{name} ({:.1} Mo)", file.display(), REMOTE_ROOT.join("/"), run.name, size as f64 / 1e6);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_segments_like_encode_uri_component() {
        assert_eq!(encode_segment("vidéo finale (1).mp4"), "vid%C3%A9o%20finale%20(1).mp4");
        assert_eq!(encode_segment("a/b?c"), "a%2Fb%3Fc");
    }
}
