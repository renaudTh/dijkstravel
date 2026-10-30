//! Plusieurs rendus à la suite, chacun avec sa tournée et sa caméra. Les segments se chevauchent le
//! temps d'un fondu : la fin de l'un s'efface sous le début du suivant, sans trajet entre les deux.

use skia_safe::Canvas;

use crate::model::Video;
use crate::render::{Frames, Renderer};

pub struct Montage {
    parts: Vec<Renderer>,
    /// Première image de chaque segment dans la vidéo.
    starts: Vec<u32>,
    fade: u32,
}

impl Montage {
    /// `parts` : au moins un rendu, tous au même format. `fade` : images du fondu, bornées par le plus
    /// court segment.
    pub fn new(parts: Vec<Renderer>, fade: u32) -> Montage {
        assert!(!parts.is_empty(), "un montage sans segment");
        let (starts, fade) = layout(&parts.iter().map(Frames::frame_count).collect::<Vec<_>>(), fade);
        Montage { parts, starts, fade }
    }

    /// Le segment qui commence le plus tard avant `frame`, et l'image `frame` dans ce segment.
    fn locate(&self, frame: u32) -> (usize, u32) {
        let index = self.starts.partition_point(|&start| start <= frame).saturating_sub(1);
        (index, frame - self.starts[index])
    }
}

impl Frames for Montage {
    fn video(&self) -> &Video {
        self.parts[0].video()
    }

    fn frame_count(&self) -> u32 {
        let last = self.parts.len() - 1;
        self.starts[last] + self.parts[last].frame_count()
    }

    fn needed_tiles(&self, frames: std::ops::Range<u32>) -> Vec<String> {
        let mut tiles: Vec<String> = self
            .parts
            .iter()
            .zip(&self.starts)
            .flat_map(|(part, &start)| {
                let local = frames.start.saturating_sub(start)..frames.end.saturating_sub(start).min(part.frame_count());
                part.needed_tiles(local)
            })
            .collect();
        tiles.sort();
        tiles.dedup();
        tiles
    }

    fn draw(&self, canvas: &Canvas, frame: u32, with_hud: bool) {
        let (index, local) = self.locate(frame);
        if index == 0 || local >= self.fade {
            self.parts[index].draw(canvas, local, with_hud);
            return;
        }
        // Les cartes se fondent ; une seule carte d'infos, celle qui domine, pour ne pas mêler deux textes.
        let (previous, opacity) = (index - 1, (local + 1) as f32 / (self.fade + 1) as f32);
        let previous_local = frame - self.starts[previous];
        self.parts[previous].draw(canvas, previous_local, false);
        canvas.save_layer_alpha_f(None, opacity);
        self.parts[index].draw(canvas, local, false);
        canvas.restore();
        if with_hud {
            if opacity < 0.5 {
                self.parts[previous].draw_hud(canvas, previous_local);
            } else {
                self.parts[index].draw_hud(canvas, local);
            }
        }
    }
}

/// Première image de chaque segment, de `counts` images, et fondu retenu : chaque segment commence
/// `fade` images avant la fin du précédent.
fn layout(counts: &[u32], fade: u32) -> (Vec<u32>, u32) {
    let fade = counts.iter().map(|count| count.saturating_sub(1)).min().unwrap_or(0).min(fade);
    let mut starts = Vec::with_capacity(counts.len());
    let mut next = 0;
    for count in counts {
        starts.push(next);
        next += count - fade;
    }
    (starts, fade)
}

#[cfg(test)]
mod tests {
    use super::layout;

    #[test]
    fn segments_overlap_by_the_fade() {
        assert_eq!(layout(&[240, 240, 210], 15), (vec![0, 225, 450], 15));
        // Pas plus long que le plus court segment.
        assert_eq!(layout(&[240, 4], 15), (vec![0, 237], 3));
        assert_eq!(layout(&[90], 15), (vec![0], 15));
    }
}
