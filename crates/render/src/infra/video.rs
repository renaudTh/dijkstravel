//! Vidéo : les images sont rendues en parallèle, converties en YUV 4:2:0 dans leur thread, puis écrites
//! dans l'ordre sur l'entrée d'un seul ffmpeg. Il encode sur la puce vidéo de la carte NVIDIA (NVENC)
//! quand elle répond, ce qui laisse le processeur au rendu ; sinon par x264, réparti sur les cœurs.
//! `DIJKSTRAVEL_ENCODER=x264` force x264.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Condvar, Mutex, OnceLock, mpsc};

use skia_safe::ColorType;

use crate::monitor::{CANCELLED, Monitor, Stage};
use crate::render::Frames;

/// Images rendues d'avance par thread au plus : borne la mémoire quand une image tarde.
const AHEAD_PER_THREAD: u32 = 2;
/// Côté le plus long que NVENC encode en H.264.
const NVENC_MAX_SIDE: u32 = 4096;

/// Réglages de l'encodeur H.264 pour une vidéo `width × height`.
fn encoder(width: u32, height: u32) -> Vec<&'static str> {
    // Mêmes réglages que l'ancien rendu (Chromium) : qualité et taille de fichier inchangées.
    const X264: &str = "-c:v libx264 -preset veryfast -crf 23";
    // Même qualité que x264 `-crf 23` (SSIM égale sur un passage en relief 4K), fichier un quart plus lourd.
    const NVENC: &str = "-c:v h264_nvenc -preset p5 -tune hq -rc vbr -cq 30 -b:v 0 -bf 3 -spatial-aq 1 \
                         -temporal-aq 1 -profile:v high";
    let settings = if width.max(height) <= NVENC_MAX_SIDE && nvenc() { NVENC } else { X264 };
    settings.split_whitespace().collect()
}

/// NVENC répond : ffmpeg l'a, la carte et son pilote aussi. Essayé une fois par processus.
fn nvenc() -> bool {
    static WORKS: OnceLock<bool> = OnceLock::new();
    *WORKS.get_or_init(|| {
        if std::env::var("DIJKSTRAVEL_ENCODER").is_ok_and(|encoder| encoder == "x264") {
            return false;
        }
        Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", "color=c=black:s=256x256:r=1", "-frames:v", "1"])
            .args(["-c:v", "h264_nvenc", "-f", "null", "-"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Les `frames` premières images, encodées dans `out` (MP4 H.264) ; `with_hud` : avec la carte d'infos.
pub fn encode_video(
    renderer: &dyn Frames,
    frames: u32,
    with_hud: bool,
    threads: usize,
    out: &str,
    monitor: &dyn Monitor,
) -> Result<(), String> {
    let (width, height, fps) = (renderer.video().width, renderer.video().height, renderer.video().fps);
    let mut ffmpeg = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "yuv420p"])
        .args(["-s", &format!("{width}x{height}"), "-r", &fps.to_string(), "-i", "-"])
        .args(encoder(width, height))
        .args(["-pix_fmt", "yuv420p"])
        .args(["-movflags", "+faststart", out])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg : {e}"))?;
    let mut stdin = ffmpeg.stdin.take().ok_or("ffmpeg : pas d'entrée")?;

    let next = AtomicU32::new(0);
    // Images écrites jusqu'ici : un thread n'en prend une nouvelle que dans la fenêtre qui suit.
    let written = (Mutex::new(0u32), Condvar::new());
    let window = AHEAD_PER_THREAD * threads as u32;
    let (sender, receiver) = mpsc::channel::<(u32, Vec<u8>)>();
    let result = std::thread::scope(|scope| {
        for _ in 0..threads {
            let sender = sender.clone();
            let (next, written) = (&next, &written);
            scope.spawn(move || {
                let mut surface = renderer.surface();
                loop {
                    let frame = next.fetch_add(1, Ordering::SeqCst);
                    if frame >= frames || monitor.cancelled() {
                        break;
                    }
                    {
                        let (count, changed) = written;
                        let mut count = count.lock().unwrap();
                        while frame >= *count + window {
                            count = changed.wait(count).unwrap();
                        }
                    }
                    renderer.draw(surface.canvas(), frame, with_hud);
                    let pixels = surface.peek_pixels().expect("pixels de la surface");
                    let yuv = to_yuv420(
                        pixels.bytes().expect("pixels"),
                        pixels.row_bytes(),
                        pixels.color_type(),
                        width,
                        height,
                    );
                    if sender.send((frame, yuv)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut pending = BTreeMap::new();
        let mut done = 0u32;
        for (frame, yuv) in receiver.iter() {
            pending.insert(frame, yuv);
            while let Some(yuv) = pending.remove(&done) {
                stdin.write_all(&yuv).map_err(|e| format!("ffmpeg : {e}"))?;
                done += 1;
                let (count, changed) = &written;
                *count.lock().unwrap() = done;
                changed.notify_all();
                if done < frames {
                    monitor.progress(Stage::Frames, u64::from(done), u64::from(frames));
                }
            }
        }
        Ok::<(), String>(())
    });
    drop(stdin);
    let status = ffmpeg.wait().map_err(|e| format!("ffmpeg : {e}"))?;
    result?;
    if monitor.cancelled() {
        return Err(CANCELLED.to_owned());
    }
    if !status.success() {
        return Err(format!("ffmpeg a échoué ({status})"));
    }
    monitor.progress(Stage::Frames, u64::from(frames), u64::from(frames));
    Ok(())
}

/// RVB → YUV 4:2:0 BT.601 en plage limitée, la conversion par défaut de ffmpeg : couleurs identiques à
/// celles des vidéos rendues jusqu'ici. `pixels` : image opaque, RGBA ou BGRA selon `color_type`.
fn to_yuv420(pixels: &[u8], row_bytes: usize, color_type: ColorType, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    // Formats vidéo de `src/run.ts` : côtés pairs, un échantillon de chrominance par carré de 2 × 2.
    assert!(w % 2 == 0 && h % 2 == 0, "dimensions paires attendues");
    // Indices du rouge et du bleu dans un pixel.
    let (ri, bi) = if color_type == ColorType::BGRA8888 { (2, 0) } else { (0, 2) };
    let (cw, ch) = (w / 2, h / 2);
    let mut yuv = vec![0u8; w * h + 2 * cw * ch];
    let (luma, chroma) = yuv.split_at_mut(w * h);
    let (u_plane, v_plane) = chroma.split_at_mut(cw * ch);
    let luma_of = |p: &[u8]| {
        let (r, g, b) = (i32::from(p[ri]), i32::from(p[1]), i32::from(p[bi]));
        (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8
    };
    for cy in 0..ch {
        let top = &pixels[2 * cy * row_bytes..][..w * 4];
        let bottom = &pixels[(2 * cy + 1) * row_bytes..][..w * 4];
        let (luma_top, luma_bottom) = luma[2 * cy * w..(2 * cy + 2) * w].split_at_mut(w);
        let u_row = &mut u_plane[cy * cw..(cy + 1) * cw];
        let v_row = &mut v_plane[cy * cw..(cy + 1) * cw];
        for cx in 0..cw {
            let (a, b) = (&top[cx * 8..cx * 8 + 8], &bottom[cx * 8..cx * 8 + 8]);
            luma_top[2 * cx] = luma_of(&a[..4]);
            luma_top[2 * cx + 1] = luma_of(&a[4..]);
            luma_bottom[2 * cx] = luma_of(&b[..4]);
            luma_bottom[2 * cx + 1] = luma_of(&b[4..]);
            let sum = |i: usize| i32::from(a[i]) + i32::from(a[i + 4]) + i32::from(b[i]) + i32::from(b[i + 4]);
            let (r, g, b) = ((sum(ri) + 2) / 4, (sum(1) + 2) / 4, (sum(bi) + 2) / 4);
            u_row[cx] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128) as u8;
            v_row[cx] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128) as u8;
        }
    }
    yuv
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_safe::{Color, surfaces};

    #[test]
    fn converts_like_ffmpeg_bt601_limited() {
        let mut surface = surfaces::raster_n32_premul((2, 2)).unwrap();
        let mut convert = |color: Color| {
            surface.canvas().clear(color);
            let pixels = surface.peek_pixels().unwrap();
            to_yuv420(pixels.bytes().unwrap(), pixels.row_bytes(), pixels.color_type(), 2, 2)
        };
        assert_eq!(convert(Color::WHITE), vec![235, 235, 235, 235, 128, 128]);
        assert_eq!(convert(Color::BLACK), vec![16, 16, 16, 16, 128, 128]);
        // Rouge pur : Y 82, U 90, V 240 (BT.601, plage limitée).
        assert_eq!(convert(Color::RED), vec![82, 82, 82, 82, 90, 240]);
    }
}
