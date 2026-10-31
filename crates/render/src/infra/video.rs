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
    // `p4` : qualité de `p5` (avec ou sans `-temporal-aq`, sans effet ici), une fois et demie plus vite ;
    // en 4K60, `p5` était ce qui freinait le rendu.
    const NVENC: &str = "-c:v h264_nvenc -preset p4 -tune hq -rc vbr -cq 30 -b:v 0 -bf 3 -spatial-aq 1 -profile:v high";
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
    // Indices du rouge et du bleu dans un pixel, constants à la compilation : les boucles se vectorisent.
    let bgra = color_type == ColorType::BGRA8888;
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY : le processeur a AVX2, vérifié juste au-dessus.
        return unsafe {
            if bgra {
                convert_avx2::<2, 0>(pixels, row_bytes, width, height)
            } else {
                convert_avx2::<0, 2>(pixels, row_bytes, width, height)
            }
        };
    }
    if bgra {
        convert::<2, 0>(pixels, row_bytes, width, height)
    } else {
        convert::<0, 2>(pixels, row_bytes, width, height)
    }
}

/// `convert` compilée pour AVX2 : les mêmes boucles, vectorisées sur 256 bits au lieu de 128.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn convert_avx2<const R: usize, const B: usize>(
    pixels: &[u8],
    row_bytes: usize,
    width: u32,
    height: u32,
) -> Vec<u8> {
    convert::<R, B>(pixels, row_bytes, width, height)
}

#[inline(always)]
fn convert<const R: usize, const B: usize>(pixels: &[u8], row_bytes: usize, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    // Formats vidéo de `src/run.ts` : côtés pairs, un échantillon de chrominance par carré de 2 × 2.
    assert!(w % 2 == 0 && h % 2 == 0, "dimensions paires attendues");
    let (cw, ch) = (w / 2, h / 2);
    let mut yuv = vec![0u8; w * h + 2 * cw * ch];
    let (luma, chroma) = yuv.split_at_mut(w * h);
    let (u_plane, v_plane) = chroma.split_at_mut(cw * ch);
    let lines = luma.chunks_exact_mut(2 * w).zip(u_plane.chunks_exact_mut(cw)).zip(v_plane.chunks_exact_mut(cw));
    for (cy, ((luma, u_row), v_row)) in lines.enumerate() {
        let top = &pixels[2 * cy * row_bytes..][..w * 4];
        let bottom = &pixels[(2 * cy + 1) * row_bytes..][..w * 4];
        let (luma_top, luma_bottom) = luma.split_at_mut(w);
        luma_row::<R, B>(top, luma_top);
        luma_row::<R, B>(bottom, luma_bottom);
        let pairs = top.as_chunks::<8>().0.iter().zip(bottom.as_chunks::<8>().0);
        for ((u, v), (a, b)) in u_row.iter_mut().zip(v_row.iter_mut()).zip(pairs) {
            // Chaque canal des deux pixels de chaque ligne, additionné sur 32 bits.
            let pixel = |row: &[u8; 8], i: usize| {
                u32::from_le_bytes([row[4 * i], row[4 * i + 1], row[4 * i + 2], row[4 * i + 3]])
            };
            let channel = |shift: usize| {
                let of = |p: u32| ((p >> (8 * shift)) & 0xff) as i32;
                (of(pixel(a, 0)) + of(pixel(a, 1)) + of(pixel(b, 0)) + of(pixel(b, 1)) + 2) >> 2
            };
            let (r, g, b) = (channel(R), channel(1), channel(B));
            *u = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128) as u8;
            *v = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128) as u8;
        }
    }
    yuv
}

/// Luminance d'une ligne, chaque pixel lu comme un entier de 32 bits : la boucle se vectorise.
#[inline(always)]
fn luma_row<const R: usize, const B: usize>(pixels: &[u8], out: &mut [u8]) {
    let (pixels, _) = pixels.as_chunks::<4>();
    for (y, p) in out.iter_mut().zip(pixels) {
        let p = u32::from_le_bytes(*p);
        let (r, g, b) = ((p >> (8 * R)) & 0xff, (p >> 8) & 0xff, (p >> (8 * B)) & 0xff);
        *y = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
    }
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

    /// La formule pixel par pixel, telle qu'écrite avant la vectorisation.
    fn reference(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
        let (ri, bi) = (2, 0);
        let (cw, ch) = (w / 2, h / 2);
        let mut yuv = vec![0u8; w * h + 2 * cw * ch];
        let at = |x: usize, y: usize, c: usize| i32::from(pixels[(y * w + x) * 4 + c]);
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = (at(x, y, ri), at(x, y, 1), at(x, y, bi));
                yuv[y * w + x] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
            }
        }
        for cy in 0..ch {
            for cx in 0..cw {
                let sum = |c| {
                    at(2 * cx, 2 * cy, c)
                        + at(2 * cx + 1, 2 * cy, c)
                        + at(2 * cx, 2 * cy + 1, c)
                        + at(2 * cx + 1, 2 * cy + 1, c)
                };
                let (r, g, b) = ((sum(ri) + 2) / 4, (sum(1) + 2) / 4, (sum(bi) + 2) / 4);
                yuv[w * h + cy * cw + cx] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128) as u8;
                yuv[w * h + cw * ch + cy * cw + cx] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128) as u8;
            }
        }
        yuv
    }

    #[test]
    fn converts_every_pixel_like_the_formula() {
        let (w, h) = (38, 6);
        // Octets pseudo-aléatoires, alpha opaque.
        let mut seed = 12345u32;
        let pixels: Vec<u8> = (0..w * h * 4)
            .map(|i| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                if i % 4 == 3 { 255 } else { (seed >> 16) as u8 }
            })
            .collect();
        assert_eq!(to_yuv420(&pixels, w * 4, ColorType::BGRA8888, w as u32, h as u32), reference(&pixels, w, h));
    }
}
