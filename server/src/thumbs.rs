//! Scaled-down copies of game media (`?w=<pixels>` on the media routes). Clients ask for images
//! at the size they show them (a vault cover is a few hundred pixels wide), which saves them
//! downloading and decoding the full image. The copies are kept in `<database>/thumbs`.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tokio::sync::Semaphore;

/// Widths are rounded up to a multiple of this, so few copies of an image are kept.
const STEP: u32 = 64;
const MAX_WIDTH: u32 = 1920;
const JPEG_QUALITY: u8 = 80;

/// Images scaled at the same time (decoding and scaling is CPU work).
static SLOTS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 8)));

/// The `w` query parameter, rounded up to a multiple of STEP. None: send the original.
pub fn width_param(w: Option<&String>) -> Option<u32> {
    let w: u32 = w?.parse().ok().filter(|w| *w > 0)?;
    Some(w.div_ceil(STEP).saturating_mul(STEP).min(MAX_WIDTH))
}

/// The file of `src` scaled down to `width` pixels, made when missing or older than `src`. None
/// when the image is not wider than that (the original is sent).
pub async fn scaled(src: &Path, thumbs_dir: &Path, game_id: i64, name: &str, width: u32) -> anyhow::Result<Option<PathBuf>> {
    // JPEG for photos; PNG and WebP may be transparent and become PNG.
    let jpeg = name.to_ascii_lowercase().ends_with(".jpg") || name.to_ascii_lowercase().ends_with(".jpeg");
    let out = thumbs_dir.join(game_id.to_string()).join(format!("{name}.{width}.{}", if jpeg { "jpg" } else { "png" }));
    let src_time = tokio::fs::metadata(src).await?.modified()?;
    if let Ok(m) = tokio::fs::metadata(&out).await
        && m.modified()? >= src_time
    {
        return Ok(Some(out));
    }
    let _slot = SLOTS.acquire().await?;
    let (src, out) = (src.to_path_buf(), out.clone());
    tokio::task::spawn_blocking(move || -> anyhow::Result<Option<PathBuf>> {
        // Made by another request while this one waited.
        if std::fs::metadata(&out).and_then(|m| m.modified()).is_ok_and(|t| t >= src_time) {
            return Ok(Some(out));
        }
        let (w, _) = image::image_dimensions(&src)?;
        if w <= width {
            return Ok(None);
        }
        let img = image::open(&src)?.resize(width, u32::MAX, image::imageops::FilterType::CatmullRom);
        std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")))?;
        let tmp = out.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        let written = (|| -> anyhow::Result<()> {
            let file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
            if jpeg {
                img.to_rgb8().write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(file, JPEG_QUALITY))?;
            } else {
                img.write_with_encoder(image::codecs::png::PngEncoder::new(file))?;
            }
            std::fs::rename(&tmp, &out)?;
            Ok(())
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written.map(|_| Some(out))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::{scaled, width_param};

    #[test]
    fn widths_are_rounded_up_and_limited() {
        assert_eq!(width_param(Some(&"376".into())), Some(384));
        assert_eq!(width_param(Some(&"384".into())), Some(384));
        assert_eq!(width_param(Some(&"99999".into())), Some(1920));
        assert_eq!(width_param(Some(&"0".into())), None);
        assert_eq!(width_param(Some(&"x".into())), None);
        assert_eq!(width_param(None), None);
    }

    #[tokio::test]
    async fn images_are_scaled_once_and_small_ones_are_sent_as_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("cover.jpg");
        image::RgbImage::from_pixel(528, 704, image::Rgb([200, 30, 30])).save(&src).unwrap();
        let thumbs = dir.path().join("thumbs");

        let out = scaled(&src, &thumbs, 7, "cover.jpg", 256).await.unwrap().expect("scaled");
        assert_eq!(out, thumbs.join("7").join("cover.jpg.256.jpg"));
        assert_eq!(image::image_dimensions(&out).unwrap(), (256, 341));
        let made = std::fs::metadata(&out).unwrap().modified().unwrap();
        // Asked again: the same file, not made again.
        assert_eq!(scaled(&src, &thumbs, 7, "cover.jpg", 256).await.unwrap(), Some(out.clone()));
        assert_eq!(std::fs::metadata(&out).unwrap().modified().unwrap(), made);

        assert_eq!(scaled(&src, &thumbs, 7, "cover.jpg", 640).await.unwrap(), None, "not wider than asked for");
    }
}
