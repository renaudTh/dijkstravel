//! L'avancement d'un rendu dans le terminal, sur une ligne réécrite : `tuiles n / N`, `image n / N`.

use std::io::Write;

use dijkstravel_render::monitor::{Monitor, Stage};

pub struct Console {
    /// Images par seconde de la vidéo : une ligne `image` par seconde rendue.
    pub fps: u32,
}

impl Monitor for Console {
    fn progress(&self, stage: Stage, done: u64, total: u64) {
        match stage {
            Stage::Tiles if total == 0 => {}
            Stage::Tiles if done == total => println!("\rtuiles {done} / {total}"),
            Stage::Tiles if done.is_multiple_of(50) => {
                print!("\rtuiles {done} / {total}");
                let _ = std::io::stdout().flush();
            }
            Stage::Frames if done == total => {
                eprintln!("\rimage {done} / {total}");
            }
            Stage::Frames if done.is_multiple_of(u64::from(self.fps)) => eprint!("\rimage {done} / {total}"),
            _ => {}
        }
    }
}
