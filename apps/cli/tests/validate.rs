//! `dijkstravel validate` sur un run écrit dans un dossier temporaire.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("dijkstravel-{name}-{}", std::process::id()));
        let run = dir.join("runs").join("bretagne");
        std::fs::create_dir_all(&run).unwrap();
        let raw = json!({
            "name": "bretagne",
            "source": { "kind": "manuel", "description": "test", "date": "2026-10-27", "licence": "CC0" },
            "bbox": { "west": -5.2, "south": 47.2, "east": -1.0, "north": 48.95 },
            "mode": "loop"
        });
        std::fs::write(run.join("run.json"), raw.to_string()).unwrap();
        Scratch(dir)
    }

    fn places(&self, features: &[Value]) {
        let text = json!({ "type": "FeatureCollection", "features": features }).to_string();
        std::fs::write(self.0.join("runs/bretagne/places.geojson"), text).unwrap();
    }

    fn validate(&self, arg: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_dijkstravel")).arg("validate").arg(arg).current_dir(&self.0).output().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn point(id: &str, name: &str, coordinates: [f64; 2]) -> Value {
    json!({ "type": "Feature", "geometry": { "type": "Point", "coordinates": coordinates }, "properties": { "id": id, "name": name } })
}

fn stderr_lines(output: &Output) -> usize {
    String::from_utf8_lossy(&output.stderr).lines().count()
}

#[test]
fn accepts_a_conforming_places_file_from_the_folder_or_the_file() {
    let scratch = Scratch::new("ok");
    scratch.places(&[point("a", "Rennes", [-1.68, 48.11]), point("b", "Brest", [-4.49, 48.39])]);
    for arg in ["runs/bretagne", "runs/bretagne/run.json"] {
        let output = scratch.validate(arg);
        assert!(output.status.success(), "{arg}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("2 lieux conformes"));
    }
}

#[test]
fn prints_every_issue_of_a_faulty_places_file() {
    let scratch = Scratch::new("faulty");
    scratch.places(&[
        point("a", "", [-1.68, 48.11]),
        point("b", "Brest", [-4.49, 48.39]),
        point("c", "Paris", [2.35, 48.85]),
    ]);
    let output = scratch.validate("runs/bretagne");
    assert!(!output.status.success());
    assert_eq!(stderr_lines(&output), 2);
}

#[test]
fn reports_a_missing_run() {
    let scratch = Scratch::new("missing");
    let output = scratch.validate("runs/absent");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(&Path::new("runs/absent/run.json").display().to_string()));
}
