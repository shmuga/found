//! Extract scanned images from PDFs. PDFs from scanners usually wrap one full-resolution image per
//! page: a JPEG stream (`DCTDecode`, copied byte-for-byte, no re-encode) or a Flate-compressed
//! bitmap (written as lossless PNG). Pages with several images yield the largest one.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Page {
    pub page: u32,
    pub path: PathBuf,
}

pub fn is_pdf(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Write each page's image into `out_dir` as `<stem>.jpg|png` (or `<stem>-p01…` for
/// multi-page PDFs). Never overwrites: adds a numeric suffix. Returns written files; pages
/// whose images can't be extracted are reported in the error only if nothing was extracted.
pub fn extract(pdf: &Path, out_dir: &Path) -> Result<Vec<Page>> {
    let doc = lopdf::Document::load(pdf).with_context(|| format!("read {}", pdf.display()))?;
    let stem = pdf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("scan")
        .to_string();
    let pages = doc.get_pages();
    let multi = pages.len() > 1;
    let mut out = vec![];
    let mut problems = vec![];
    for (n, pid) in pages {
        let imgs = match doc.get_page_images(pid) {
            Ok(v) => v,
            Err(e) => {
                problems.push(format!("page {n}: {e}"));
                continue;
            }
        };
        let Some(img) = imgs.iter().max_by_key(|i| i.width * i.height) else {
            problems.push(format!("page {n}: no image"));
            continue;
        };
        let name = if multi {
            format!("{stem}-p{n:02}")
        } else {
            stem.clone()
        };
        let filters: Vec<String> = img.filters.clone().unwrap_or_default();
        let fl: Vec<&str> = filters.iter().map(|s| s.as_str()).collect();
        let res = match fl.as_slice() {
            ["DCTDecode"] => write_unique(out_dir, &name, "jpg", img.content),
            [] | ["FlateDecode"] => {
                let data = if fl.is_empty() {
                    img.content.to_vec()
                } else {
                    doc.get_object(img.id)
                        .and_then(|o| o.as_stream())
                        .and_then(|s| s.decompressed_content())
                        .map_err(|e| anyhow::anyhow!("inflate: {e}"))?
                };
                let invert = decode_inverted(img.origin_dict);
                raw_to_png(
                    &data,
                    img.width as u32,
                    img.height as u32,
                    img.bits_per_component.unwrap_or(8) as u32,
                    invert,
                )
                .and_then(|png| write_unique(out_dir, &name, "png", &png))
            }
            other => Err(anyhow::anyhow!("unsupported image encoding {other:?}")),
        };
        match res {
            Ok(path) => out.push(Page { page: n, path }),
            Err(e) => problems.push(format!("page {n}: {e:#}")),
        }
    }
    if out.is_empty() {
        bail!(
            "no extractable images ({})",
            if problems.is_empty() {
                "empty PDF".into()
            } else {
                problems.join("; ")
            }
        );
    }
    for p in &problems {
        log::warn!("{}: {p}", pdf.display());
    }
    Ok(out)
}

fn decode_inverted(dict: &lopdf::Dictionary) -> bool {
    dict.get(b"Decode")
        .and_then(|o| o.as_array())
        .ok()
        .and_then(|a| {
            a.first().and_then(|v| {
                v.as_float()
                    .ok()
                    .or_else(|| v.as_i64().ok().map(|i| i as f32))
            })
        })
        .is_some_and(|first| first >= 1.0)
}

fn raw_to_png(data: &[u8], w: u32, h: u32, bpc: u32, invert: bool) -> Result<Vec<u8>> {
    let px = (w as usize) * (h as usize);
    if px == 0 {
        bail!("empty image");
    }
    let rgb: Vec<u8> = match bpc {
        1 => {
            let stride = (w as usize).div_ceil(8);
            if data.len() < stride * h as usize {
                bail!("short 1-bit image data");
            }
            let mut v = Vec::with_capacity(px * 3);
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let bit = (data[y * stride + x / 8] >> (7 - x % 8)) & 1;
                    let on = (bit == 1) != invert;
                    let g = if on { 255 } else { 0 };
                    v.extend([g, g, g]);
                }
            }
            v
        }
        8 | 16 => {
            let bytes = (bpc / 8) as usize;
            let comps = data.len() / (px * bytes);
            let at = |i: usize| data[i * bytes]; // high byte for 16-bit
            match comps {
                1 => (0..px)
                    .flat_map(|i| {
                        let g = if invert { 255 - at(i) } else { at(i) };
                        [g, g, g]
                    })
                    .collect(),
                3 => (0..px * 3).map(at).collect(),
                4 => (0..px)
                    .flat_map(|i| {
                        let k = 255 - at(i * 4 + 3) as u32;
                        [0, 1, 2].map(|c| ((255 - at(i * 4 + c) as u32) * k / 255) as u8)
                    })
                    .collect(),
                n => bail!("unsupported {n}-component image"),
            }
        }
        b => bail!("unsupported {b}-bit image"),
    };
    let mut png = vec![];
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &rgb,
        w,
        h,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(png)
}

fn write_unique(dir: &Path, name: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf> {
    let mut p = dir.join(format!("{name}.{ext}"));
    let mut k = 1;
    while p.exists() {
        p = dir.join(format!("{name}-{k}.{ext}"));
        k += 1;
    }
    std::fs::write(&p, bytes)?;
    Ok(p)
}
