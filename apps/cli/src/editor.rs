//! `dijkstravel editor runs/<nom>` : éditeur du thème du run. Sert la page (`editor/`, compilée par
//! `npm run build:editor`), son état (`/editor.json` : thème complété, valeurs par défaut, bornes des
//! réglages, nombre d'images), l'aperçu (`POST /preview` : `{ theme, frame }` → JPEG rendu par le moteur
//! comme la vidéo le rendra) et l'enregistrement (`PUT /theme.json`, thème validé). Une requête à la fois.

use std::io::Read;
use std::path::{Path, PathBuf};

use dijkstravel_core::Issue;
use dijkstravel_core::json::{field, to_js_string};
use dijkstravel_core::theme::{HUD_FONTS, IMAGERIES, NUMBER_RANGES, Theme};
use dijkstravel_render::frame::Timeline;
use dijkstravel_render::render::{Renderer, encode};
use serde_json::{Map, Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::load::{self, Outcome, fail};
use crate::render::Scene;

const PAGE_DIR: &str = "editor";
/// Un thème (ou une demande d'aperçu) ne pèse que quelques centaines d'octets : tout corps plus gros est refusé.
const MAX_BODY_BYTES: u64 = 64 * 1024;

type Reply = Response<std::io::Cursor<Vec<u8>>>;

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).expect("en-tête ASCII")
}

fn reply(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Reply {
    Response::from_data(body.into())
        .with_status_code(status)
        .with_header(header("content-type", content_type))
        .with_header(header("cache-control", "no-store"))
}

fn issues_reply(issues: &[Issue]) -> Reply {
    let list: Vec<Value> =
        issues.iter().map(|issue| json!({ "path": issue.path, "expected": issue.expected })).collect();
    reply(400, "application/json", Value::from(list).to_string())
}

fn read_body(request: &mut Request) -> Option<Value> {
    let mut text = String::new();
    request.as_reader().take(MAX_BODY_BYTES + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > MAX_BODY_BYTES {
        return None;
    }
    serde_json::from_str(&text).ok()
}

struct Editor {
    scene: Scene,
    theme_file: PathBuf,
    /// Le dernier rendu, gardé tant que le thème ne change pas ; sinon son atlas resservira au suivant.
    current: Option<(Theme, Renderer)>,
}

impl Editor {
    fn state(&self) -> Outcome<Value> {
        let raw = if self.theme_file.exists() { load::read_json(&self.theme_file)? } else { json!({}) };
        let (theme, issues) = match Theme::parse(&raw) {
            Ok(theme) => (theme, Vec::new()),
            Err(issues) => (Theme::default(), issues),
        };
        let ranges: Map<String, Value> =
            NUMBER_RANGES.iter().map(|&(path, min, max)| (path.to_owned(), json!([min, max]))).collect();
        let (width, height) = self.scene.run.video.format.size();
        let frames = Timeline::new(&self.scene.video()).frame_count();
        Ok(json!({
            "run": { "name": self.scene.run.name, "width": width, "height": height, "frames": frames },
            "theme": theme,
            "issues": issues.iter().map(|issue| json!({ "path": issue.path, "expected": issue.expected })).collect::<Vec<_>>(),
            "defaults": Theme::default(),
            "ranges": ranges,
            "fonts": HUD_FONTS,
            "imageries": IMAGERIES,
        }))
    }

    fn preview(&mut self, body: Option<Value>) -> Outcome<Reply> {
        let body = body.unwrap_or(Value::Null);
        let theme = Theme::parse(field(&body, "theme").unwrap_or(&Value::Null));
        let frame = field(&body, "frame").and_then(Value::as_u64).and_then(|frame| u32::try_from(frame).ok());
        let (theme, frame) = match (theme, frame) {
            (Ok(theme), Some(frame)) => (theme, frame),
            (Err(issues), _) => return Ok(issues_reply(&issues)),
            (Ok(_), None) => return Ok(issues_reply(&[Issue::new("frame", "entier >= 0")])),
        };
        let renderer = renderer_for(&mut self.current, &self.scene, &theme)?;
        self.scene.downloader.fetch_tiles(&renderer.needed_tiles(frame..frame + 1), false)?;
        let image = renderer.render(frame.min(renderer.frame_count().saturating_sub(1)), true);
        // JPEG : bien plus rapide à encoder qu'un PNG de cette taille, sans différence visible à l'écran.
        let bytes = encode(&image, "preview.jpg").ok_or("encodage impossible")?;
        Ok(reply(200, "image/jpeg", bytes))
    }

    /// Enregistre le thème envoyé, après validation : jamais de fichier écrit tel quel.
    fn save(&self, body: Option<Value>) -> Outcome<Reply> {
        match Theme::parse(&body.unwrap_or(Value::Null)) {
            Ok(theme) => {
                load::write_file(&self.theme_file, to_js_string(&theme))?;
                Ok(reply(204, "text/plain", Vec::new()))
            }
            Err(issues) => Ok(issues_reply(&issues)),
        }
    }

    fn static_file(name: &str, content_type: &str) -> Outcome<Reply> {
        let file = Path::new(PAGE_DIR).join(name);
        if !file.exists() {
            return fail(format!("{} absent : npm run build:editor", file.display()));
        }
        Ok(reply(200, content_type, std::fs::read(file)?))
    }

    fn handle(&mut self, request: &mut Request) -> Outcome<Reply> {
        let path = request.url().split('?').next().unwrap_or_default().to_owned();
        match (request.method(), path.as_str()) {
            (Method::Get, "/" | "/editor") => Self::static_file("index.html", "text/html; charset=utf-8"),
            (Method::Get, "/editor.js") => Self::static_file("dist/editor.js", "text/javascript; charset=utf-8"),
            (Method::Get, "/editor.json") => Ok(reply(200, "application/json", self.state()?.to_string())),
            (Method::Post, "/preview") => self.preview(read_body(request)),
            (Method::Put, "/theme.json") => self.save(read_body(request)),
            _ => Ok(reply(404, "text/plain", Vec::new())),
        }
    }
}

/// Le rendu de `theme`, refait seulement si le thème a changé ; la tournée et l'atlas (morceaux déjà
/// calculés compris) passent d'un rendu au suivant tant qu'ils ne dépendent d'aucun réglage changé.
fn renderer_for<'a>(current: &'a mut Option<(Theme, Renderer)>, scene: &Scene, theme: &Theme) -> Outcome<&'a Renderer> {
    if current.as_ref().is_none_or(|(known, _)| known != theme) {
        let previous = current.take().map(|(_, renderer)| renderer);
        let renderer = Renderer::new(scene.input(theme.clone())?, scene.tour.clone(), previous.as_ref());
        *current = Some((theme.clone(), renderer));
    }
    Ok(&current.as_ref().expect("rendu chargé").1)
}

pub fn main(arg: &Path) -> Outcome {
    if !Path::new(PAGE_DIR).join("dist/editor.js").exists() {
        return fail("editor/dist/editor.js absent : npm run build:editor");
    }
    let scene = Scene::load(arg)?;
    let theme_file = load::theme_file(&scene.run);
    let server = Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = server.server_addr().to_ip().map_or(0, |address| address.port());
    println!("Éditeur : http://127.0.0.1:{port}/editor  (thème {}, Ctrl+C pour arrêter)", theme_file.display());
    let mut editor = Editor { scene, theme_file, current: None };
    for mut request in server.incoming_requests() {
        let response = editor.handle(&mut request).unwrap_or_else(|load::Failure(messages)| {
            let message = messages.join("\n");
            eprintln!("{message}");
            reply(500, "text/plain; charset=utf-8", message)
        });
        let _ = request.respond(response);
    }
    Ok(())
}
