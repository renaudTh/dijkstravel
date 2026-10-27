//! `dijkstravel-render <commande> <entrée.json> …`, appelé par `src/render/native.ts`.

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;

use dijkstravel_render::model::{Input, Tour};
use dijkstravel_render::render::{Renderer, encode};
use dijkstravel_render::video::encode_video;
use serde::Deserialize;

const USAGE: &str = "Usage : dijkstravel-render tiles <entrée.json> [--frames=<n> | --frame=<n>]
       dijkstravel-render frame <entrée.json> --frame=<n> --out=<fichier.png|jpg> [--no-hud]
       dijkstravel-render video <entrée.json> --out=<fichier.mp4> [--frames=<n>] [--threads=<n>]
       dijkstravel-render serve   (requêtes JSON sur l'entrée standard, une par ligne)";

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().find_map(|arg| arg.strip_prefix(name)?.strip_prefix('='))
}

fn read_input(file: &str) -> Result<(String, Input), String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file} : {e}"))?;
    let input = serde_json::from_str(&text).map_err(|e| format!("{file} : {e}"))?;
    Ok((text, input))
}

fn read_tour(file: &str) -> Result<Tour, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file} : {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{file} : {e}"))
}

fn run(args: &[String]) -> Result<(), String> {
    if let [command] = args
        && command == "serve"
    {
        return serve();
    }
    let [command, input_file, rest @ ..] = args else { return Err(USAGE.into()) };
    let (_, input) = read_input(input_file)?;
    let tour = read_tour(&input.tour_file)?;
    let renderer = Renderer::new(input, Arc::new(tour), None);
    let frames = match option(rest, "--frames") {
        Some(n) => n.parse::<u32>().map_err(|_| USAGE.to_owned())?.min(renderer.frame_count()),
        None => renderer.frame_count(),
    };
    match command.as_str() {
        "tiles" => {
            let range = match option(rest, "--frame") {
                Some(n) => {
                    let frame: u32 = n.parse().map_err(|_| USAGE.to_owned())?;
                    frame..frame + 1
                }
                None => 0..frames,
            };
            for tile in renderer.needed_tiles(range) {
                println!("{tile}");
            }
            Ok(())
        }
        "frame" => {
            let frame: u32 = option(rest, "--frame").and_then(|n| n.parse().ok()).ok_or(USAGE)?;
            let out = option(rest, "--out").ok_or(USAGE)?;
            write_frame(&renderer, frame, out, !rest.iter().any(|arg| arg == "--no-hud"))
        }
        "video" => {
            let out = option(rest, "--out").ok_or(USAGE)?;
            let threads = match option(rest, "--threads") {
                Some(n) => n.parse::<usize>().ok().filter(|&n| n > 0).ok_or(USAGE)?,
                None => std::thread::available_parallelism().map_or(4, |n| n.get()),
            };
            encode_video(&renderer, frames, threads, out)
        }
        _ => Err(USAGE.into()),
    }
}

fn write_frame(renderer: &Renderer, frame: u32, out: &str, with_hud: bool) -> Result<(), String> {
    let image = renderer.render(frame.min(renderer.frame_count().saturating_sub(1)), with_hud);
    let bytes = encode(&image, out).ok_or("encodage impossible")?;
    std::fs::write(out, bytes).map_err(|e| format!("{out} : {e}"))
}

#[derive(Deserialize)]
struct Request {
    input: String,
    frame: u32,
    /// Absent : seulement la liste des tuiles que lira l'image, à télécharger avant de la demander.
    out: Option<String>,
}

/// Aperçu de l'éditeur : une requête `{"input", "frame", "out"}` par ligne, une réponse par ligne
/// (`ok`, `tuiles <a> <b>…` sans `out`, ou `erreur …`). La tournée et l'atlas (morceaux déjà calculés
/// compris) restent en mémoire d'une requête à l'autre tant qu'ils ne dépendent d'aucun réglage changé.
fn serve() -> Result<(), String> {
    let mut current: Option<(String, Renderer)> = None;
    let mut stdout = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let answer = |current: &mut Option<(String, Renderer)>| -> Result<String, String> {
            let request: Request = serde_json::from_str(&line).map_err(|e| format!("requête : {e}"))?;
            let (text, input) = read_input(&request.input)?;
            if current.as_ref().is_none_or(|(known, _)| *known != text) {
                let previous = current.take().map(|(_, renderer)| renderer);
                let tour = match &previous {
                    Some(previous) if previous.input.tour_file == input.tour_file => previous.tour.clone(),
                    _ => Arc::new(read_tour(&input.tour_file)?),
                };
                *current = Some((text, Renderer::new(input, tour, previous.as_ref())));
            }
            let (_, renderer) = current.as_ref().expect("rendu chargé");
            match &request.out {
                Some(out) => write_frame(renderer, request.frame, out, true).map(|()| "ok".to_owned()),
                None => Ok(format!("tuiles {}", renderer.needed_tiles(request.frame..request.frame + 1).join(" "))),
            }
        };
        let reply = answer(&mut current).unwrap_or_else(|message| format!("erreur {message}"));
        writeln!(stdout, "{reply}").and_then(|()| stdout.flush()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
