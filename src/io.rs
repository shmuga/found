//! Decoding / encoding. Large JPEG scans are decoded with DCT scaling for proxies, which is
//! several times faster than a full decode followed by a resize.

use crate::img::Img;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Default editing proxy size (user-overridable in settings).
pub const PROXY_EDGE: usize = 2400;
pub const THUMB_EDGE: usize = 320;

pub fn is_image(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "tif" | "tiff")
    )
}

fn is_jpeg(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg")
    )
}

/// Decode with the long edge reduced to at most `max_edge`.
pub fn load_scaled(path: &Path, max_edge: usize) -> Result<Img> {
    if is_jpeg(path)
        && let Ok(img) = load_jpeg_scaled(path, max_edge)
    {
        return Ok(img.fit(max_edge));
    }
    Ok(load_full(path)?.fit(max_edge))
}

fn load_jpeg_scaled(path: &Path, max_edge: usize) -> Result<Img> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut dec = jpeg_decoder::Decoder::new(std::io::BufReader::new(file));
    dec.read_info()?;
    let info = dec.info().context("no jpeg info")?;
    let long = info.width.max(info.height) as usize;
    if long > max_edge {
        // `scale` picks the smallest DCT scale that is still >= the requested size.
        let s = max_edge as f32 / long as f32;
        let rw = ((info.width as f32 * s).ceil() as u16).max(1);
        let rh = ((info.height as f32 * s).ceil() as u16).max(1);
        dec.scale(rw, rh)?;
    }
    let data = dec.decode()?;
    let info = dec.info().context("no jpeg info")?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgb: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => data,
        jpeg_decoder::PixelFormat::L8 => data.iter().flat_map(|&v| [v, v, v]).collect(),
        jpeg_decoder::PixelFormat::L16 => data
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0]])
            .collect(),
        jpeg_decoder::PixelFormat::CMYK32 => data
            .chunks_exact(4)
            .flat_map(|c| {
                // Adobe CMYK JPEGs are stored inverted.
                let k = c[3] as f32 / 255.0;
                [c[0], c[1], c[2]].map(|v| (v as f32 * k) as u8)
            })
            .collect(),
    };
    if rgb.len() != w * h * 3 {
        bail!("unexpected jpeg buffer size");
    }
    Ok(Img::from_rgb8(w, h, &rgb))
}

pub fn load_full(path: &Path) -> Result<Img> {
    let img = image::ImageReader::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .with_guessed_format()?;
    let mut img = img;
    img.no_limits();
    let dynimg = img
        .decode()
        .with_context(|| format!("decode {}", path.display()))?;
    let rgb = dynimg.to_rgb8();
    Ok(Img::from_rgb8(
        rgb.width() as usize,
        rgb.height() as usize,
        rgb.as_raw(),
    ))
}

pub fn image_size(path: &Path) -> Result<(u32, u32)> {
    Ok(image::image_dimensions(path)?)
}

pub fn save_jpeg(img: &Img, path: &Path, quality: u8) -> Result<()> {
    let tmp = path.with_extension("tmp.jpg");
    {
        let f = std::fs::File::create(&tmp)?;
        let mut w = std::io::BufWriter::new(f);
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, quality);
        let buf = img.to_rgb8();
        image::ImageEncoder::write_image(
            enc,
            &buf,
            img.w as u32,
            img.h as u32,
            image::ExtendedColorType::Rgb8,
        )?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Disk cache for thumbnails in `dir`, keyed by path + size + mtime.
pub fn thumb_cache_path(path: &Path, dir: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified().ok()?.hash(&mut h);
    THUMB_EDGE.hash(&mut h);
    Some(dir.join(format!("{:016x}.jpg", h.finish())))
}

/// Thumbnail, cached in `cache_dir` when given.
pub fn load_thumb(path: &Path, cache_dir: Option<&Path>) -> Result<Img> {
    if let Some(cp) = cache_dir.and_then(|d| thumb_cache_path(path, d)) {
        if cp.exists()
            && let Ok(img) = load_full(&cp)
        {
            return Ok(img);
        }
        let img = load_scaled(path, THUMB_EDGE)?;
        if let Some(dir) = cp.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = save_jpeg(&img, &cp, 85);
        return Ok(img);
    }
    load_scaled(path, THUMB_EDGE)
}
