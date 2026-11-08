//! Node operations. Every op is resolution independent: sizes are fractions of the image,
//! random choices are hashed from normalised coordinates + seed, so the proxy preview and the
//! full-resolution export look the same.

pub mod basic;
pub mod glitch;

use crate::img::Img;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Aspect {
    #[default]
    Free,
    Square,
    R3x2,
    R2x3,
    Original,
}

impl Aspect {
    pub const ALL: [Aspect; 5] = [
        Aspect::Free,
        Aspect::Square,
        Aspect::R3x2,
        Aspect::R2x3,
        Aspect::Original,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Aspect::Free => "Free",
            Aspect::Square => "1:1",
            Aspect::R3x2 => "3:2",
            Aspect::R2x3 => "2:3",
            Aspect::Original => "Orig",
        }
    }

    /// Pixel aspect ratio w/h, or None for free.
    pub fn ratio(self, img_w: f32, img_h: f32) -> Option<f32> {
        match self {
            Aspect::Free => None,
            Aspect::Square => Some(1.0),
            Aspect::R3x2 => Some(1.5),
            Aspect::R2x3 => Some(2.0 / 3.0),
            Aspect::Original => Some(img_w / img_h),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Crop {
    pub aspect: Aspect,
    /// Quarter turns clockwise.
    pub rot90: u8,
    /// Fine straighten angle in degrees (auto-zooms to avoid empty corners).
    pub angle: f32,
    pub flip_h: bool,
    /// Normalised rect x, y, w, h in the rotated image.
    pub rect: [f32; 4],
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            aspect: Aspect::Free,
            rot90: 0,
            angle: 0.0,
            flip_h: false,
            rect: [0.0, 0.0, 1.0, 1.0],
        }
    }
}

impl Crop {
    pub fn is_identity(&self) -> bool {
        self.rot90.is_multiple_of(4)
            && self.angle == 0.0
            && !self.flip_h
            && self.rect == [0.0, 0.0, 1.0, 1.0]
    }

    /// Size of the rotated (pre-crop) image for a given input size.
    pub fn rotated_size(&self, w: usize, h: usize) -> (usize, usize) {
        if self.rot90 % 2 == 1 { (h, w) } else { (w, h) }
    }

    /// Re-fit the rect to the aspect ratio, keeping its centre, as large as possible.
    pub fn apply_aspect(&mut self, rw: usize, rh: usize) {
        let (iw, ih) = (rw as f32, rh as f32);
        let Some(r) = self.aspect.ratio(iw, ih) else {
            return;
        };
        let [x, y, w, h] = self.rect;
        let (cx, cy) = ((x + w * 0.5) * iw, (y + h * 0.5) * ih);
        // Largest rect of ratio r inside the image.
        let mut pw = iw.min(ih * r);
        let mut ph = pw / r;
        // Prefer staying within the current crop size if it's smaller in both dims.
        let (cw, ch) = (w * iw, h * ih);
        if cw < pw && ch < ph {
            let s = (cw / pw).max(ch / ph).min(1.0);
            pw *= s;
            ph *= s;
        }
        let (pw, ph) = (pw.min(iw), ph.min(ih));
        let nx = (cx - pw * 0.5).clamp(0.0, (iw - pw).max(0.0));
        let ny = (cy - ph * 0.5).clamp(0.0, (ih - ph).max(0.0));
        self.rect = [nx / iw, ny / ih, pw / iw, ph / ih];
    }
}

/// UI-facing parameter description (mutable view into an op's fields).
pub enum Param<'a> {
    F {
        label: &'static str,
        v: &'a mut f32,
        min: f32,
        max: f32,
    },
    I {
        label: &'static str,
        v: &'a mut i32,
        min: i32,
        max: i32,
    },
    B {
        label: &'static str,
        v: &'a mut bool,
    },
    C {
        label: &'static str,
        v: &'a mut usize,
        options: &'static [&'static str],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Io,
    Basic,
    Experimental,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op")]
pub enum Op {
    Scan,
    Output,
    Crop(Crop),
    WhiteBalance {
        temp: f32,
        tint: f32,
    },
    Exposure {
        ev: f32,
    },
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
    Clarity {
        amount: f32,
    },
    Saturation {
        saturation: f32,
        vibrance: f32,
    },
    // ---- experimental ----
    RgbSplit {
        amount: f32,
        angle: f32,
    },
    PixelSort {
        lo: f32,
        hi: f32,
        vertical: bool,
        reverse: bool,
        key: usize,
    },
    SliceShift {
        bands: i32,
        shift: f32,
        chance: f32,
        seed: i32,
    },
    BlockGlitch {
        block: f32,
        chance: f32,
        mode: usize,
        seed: i32,
    },
    BitCrush {
        levels: i32,
    },
    Vhs {
        jitter: f32,
        bleed: f32,
        lines: f32,
        noise: f32,
        seed: i32,
    },
    Dither {
        levels: i32,
        scale: i32,
        mono: bool,
    },
    Pixelate {
        cell: f32,
    },
    Grain {
        amount: f32,
        size: f32,
        seed: i32,
    },
    Mirror {
        mode: usize,
    },
    ChannelSwap {
        mode: usize,
    },
    Invert {
        mix: f32,
    },
    Wave {
        amp: f32,
        freq: f32,
        vertical: bool,
        phase: f32,
    },
    Blend {
        mode: usize,
        mix: f32,
    },
    /// A where the mask is light, B where it's dark. Mask = input 3, else B's own luma.
    Mask {
        threshold: f32,
        softness: f32,
        invert: bool,
    },
    /// One image → three: R / G / B channels, or shadows / mids / highlights. The three add
    /// back up to the original (Blend › Add).
    Split {
        mode: usize,
    },
}

pub const SPLIT_MODES: &[&str] = &["Red / Green / Blue", "Shadows / Mids / Highlights"];

pub const BLEND_MODES: &[&str] = &[
    "Normal",
    "Multiply",
    "Screen",
    "Overlay",
    "Difference",
    "Add",
    "Lighten",
    "Darken",
];
pub const MIRROR_MODES: &[&str] = &[
    "Left onto right",
    "Right onto left",
    "Top onto bottom",
    "Quad",
    "Kaleido",
];
pub const SWAP_MODES: &[&str] = &["BRG", "GBR", "BGR", "RBG", "GRB"];
pub const SORT_KEYS: &[&str] = &["Luma", "Hue", "Red", "Saturation"];
pub const BLOCK_MODES: &[&str] = &["Displace", "Smear", "Channel", "Mixed"];

impl Op {
    /// Everything that can be added from the "+" menus, with defaults.
    pub fn catalog() -> Vec<Op> {
        vec![
            Op::Crop(Crop::default()),
            Op::WhiteBalance {
                temp: 0.0,
                tint: 0.0,
            },
            Op::Exposure { ev: 0.0 },
            Op::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
            },
            Op::Clarity { amount: 0.0 },
            Op::Saturation {
                saturation: 0.0,
                vibrance: 0.0,
            },
            Op::RgbSplit {
                amount: 0.6,
                angle: 0.0,
            },
            Op::PixelSort {
                lo: 0.25,
                hi: 0.8,
                vertical: false,
                reverse: false,
                key: 0,
            },
            Op::SliceShift {
                bands: 24,
                shift: 6.0,
                chance: 0.35,
                seed: 1,
            },
            Op::BlockGlitch {
                block: 3.0,
                chance: 0.12,
                mode: 3,
                seed: 1,
            },
            Op::BitCrush { levels: 6 },
            Op::Vhs {
                jitter: 0.4,
                bleed: 0.5,
                lines: 0.35,
                noise: 0.15,
                seed: 1,
            },
            Op::Dither {
                levels: 2,
                scale: 2,
                mono: false,
            },
            Op::Pixelate { cell: 1.5 },
            Op::Grain {
                amount: 0.25,
                size: 1.0,
                seed: 1,
            },
            Op::Mirror { mode: 0 },
            Op::ChannelSwap { mode: 0 },
            Op::Invert { mix: 1.0 },
            Op::Wave {
                amp: 1.5,
                freq: 6.0,
                vertical: false,
                phase: 0.0,
            },
            Op::Blend { mode: 4, mix: 1.0 },
            Op::Mask {
                threshold: 0.6,
                softness: 0.15,
                invert: false,
            },
            Op::Split { mode: 0 },
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Op::Scan => "Scan",
            Op::Output => "Output",
            Op::Crop(_) => "Crop",
            Op::WhiteBalance { .. } => "White Balance",
            Op::Exposure { .. } => "Exposure",
            Op::BrightnessContrast { .. } => "Brightness/Contrast",
            Op::Clarity { .. } => "Clarity",
            Op::Saturation { .. } => "Saturation",
            Op::RgbSplit { .. } => "RGB Split",
            Op::PixelSort { .. } => "Pixel Sort",
            Op::SliceShift { .. } => "Slice Shift",
            Op::BlockGlitch { .. } => "Block Glitch",
            Op::BitCrush { .. } => "Bit Crush",
            Op::Vhs { .. } => "VHS",
            Op::Dither { .. } => "Dither",
            Op::Pixelate { .. } => "Pixelate",
            Op::Grain { .. } => "Grain",
            Op::Mirror { .. } => "Mirror",
            Op::ChannelSwap { .. } => "Channel Swap",
            Op::Invert { .. } => "Invert",
            Op::Wave { .. } => "Wave",
            Op::Blend { .. } => "Blend",
            Op::Mask { .. } => "Mask",
            Op::Split { .. } => "Split",
        }
    }

    pub fn category(&self) -> Category {
        match self {
            Op::Scan | Op::Output => Category::Io,
            Op::Crop(_)
            | Op::WhiteBalance { .. }
            | Op::Exposure { .. }
            | Op::BrightnessContrast { .. }
            | Op::Clarity { .. }
            | Op::Saturation { .. } => Category::Basic,
            _ => Category::Experimental,
        }
    }

    pub fn inputs(&self) -> usize {
        match self {
            Op::Scan => 0,
            Op::Blend { .. } => 2,
            Op::Mask { .. } => 3,
            _ => 1,
        }
    }

    pub fn outputs(&self) -> usize {
        match self {
            Op::Output => 0,
            Op::Split { .. } => 3,
            _ => 1,
        }
    }

    /// Pin labels for the patch view ("" = unlabelled).
    pub fn input_label(&self, i: usize) -> &'static str {
        match (self, i) {
            (Op::Blend { .. }, 0) | (Op::Mask { .. }, 0) => "A",
            (Op::Blend { .. }, _) | (Op::Mask { .. }, 1) => "B",
            (Op::Mask { .. }, _) => "mask",
            _ => "in",
        }
    }

    pub fn output_label(&self, i: usize) -> &'static str {
        match (self, i) {
            (Op::Split { mode: 1 }, 0) => "shadows",
            (Op::Split { mode: 1 }, 1) => "mids",
            (Op::Split { mode: 1 }, _) => "highs",
            (Op::Split { .. }, 0) => "R",
            (Op::Split { .. }, 1) => "G",
            (Op::Split { .. }, _) => "B",
            _ => "",
        }
    }

    pub fn params(&mut self) -> Vec<Param<'_>> {
        use Param::*;
        match self {
            Op::Scan | Op::Output | Op::Crop(_) => vec![],
            Op::WhiteBalance { temp, tint } => vec![
                F {
                    label: "Temp",
                    v: temp,
                    min: -100.0,
                    max: 100.0,
                },
                F {
                    label: "Tint",
                    v: tint,
                    min: -100.0,
                    max: 100.0,
                },
            ],
            Op::Exposure { ev } => vec![F {
                label: "EV",
                v: ev,
                min: -3.0,
                max: 3.0,
            }],
            Op::BrightnessContrast {
                brightness,
                contrast,
            } => vec![
                F {
                    label: "Brightness",
                    v: brightness,
                    min: -100.0,
                    max: 100.0,
                },
                F {
                    label: "Contrast",
                    v: contrast,
                    min: -100.0,
                    max: 100.0,
                },
            ],
            Op::Clarity { amount } => vec![F {
                label: "Clarity",
                v: amount,
                min: -100.0,
                max: 100.0,
            }],
            Op::Saturation {
                saturation,
                vibrance,
            } => vec![
                F {
                    label: "Saturation",
                    v: saturation,
                    min: -100.0,
                    max: 100.0,
                },
                F {
                    label: "Vibrance",
                    v: vibrance,
                    min: -100.0,
                    max: 100.0,
                },
            ],
            Op::RgbSplit { amount, angle } => vec![
                F {
                    label: "Amount %",
                    v: amount,
                    min: 0.0,
                    max: 5.0,
                },
                F {
                    label: "Angle",
                    v: angle,
                    min: -180.0,
                    max: 180.0,
                },
            ],
            Op::PixelSort {
                lo,
                hi,
                vertical,
                reverse,
                key,
            } => vec![
                F {
                    label: "Low",
                    v: lo,
                    min: 0.0,
                    max: 1.0,
                },
                F {
                    label: "High",
                    v: hi,
                    min: 0.0,
                    max: 1.0,
                },
                C {
                    label: "Sort by",
                    v: key,
                    options: SORT_KEYS,
                },
                B {
                    label: "Vertical",
                    v: vertical,
                },
                B {
                    label: "Reverse",
                    v: reverse,
                },
            ],
            Op::SliceShift {
                bands,
                shift,
                chance,
                seed,
            } => vec![
                I {
                    label: "Bands",
                    v: bands,
                    min: 2,
                    max: 200,
                },
                F {
                    label: "Shift %",
                    v: shift,
                    min: 0.0,
                    max: 50.0,
                },
                F {
                    label: "Chance",
                    v: chance,
                    min: 0.0,
                    max: 1.0,
                },
                I {
                    label: "Seed",
                    v: seed,
                    min: 0,
                    max: 9999,
                },
            ],
            Op::BlockGlitch {
                block,
                chance,
                mode,
                seed,
            } => vec![
                F {
                    label: "Block %",
                    v: block,
                    min: 0.5,
                    max: 20.0,
                },
                F {
                    label: "Chance",
                    v: chance,
                    min: 0.0,
                    max: 1.0,
                },
                C {
                    label: "Mode",
                    v: mode,
                    options: BLOCK_MODES,
                },
                I {
                    label: "Seed",
                    v: seed,
                    min: 0,
                    max: 9999,
                },
            ],
            Op::BitCrush { levels } => vec![I {
                label: "Levels",
                v: levels,
                min: 2,
                max: 64,
            }],
            Op::Vhs {
                jitter,
                bleed,
                lines,
                noise,
                seed,
            } => vec![
                F {
                    label: "Jitter",
                    v: jitter,
                    min: 0.0,
                    max: 1.0,
                },
                F {
                    label: "Bleed",
                    v: bleed,
                    min: 0.0,
                    max: 1.0,
                },
                F {
                    label: "Lines",
                    v: lines,
                    min: 0.0,
                    max: 1.0,
                },
                F {
                    label: "Noise",
                    v: noise,
                    min: 0.0,
                    max: 1.0,
                },
                I {
                    label: "Seed",
                    v: seed,
                    min: 0,
                    max: 9999,
                },
            ],
            Op::Dither {
                levels,
                scale,
                mono,
            } => vec![
                I {
                    label: "Levels",
                    v: levels,
                    min: 2,
                    max: 16,
                },
                I {
                    label: "Dot scale",
                    v: scale,
                    min: 1,
                    max: 16,
                },
                B {
                    label: "Mono",
                    v: mono,
                },
            ],
            Op::Pixelate { cell } => vec![F {
                label: "Cell %",
                v: cell,
                min: 0.1,
                max: 20.0,
            }],
            Op::Grain { amount, size, seed } => vec![
                F {
                    label: "Amount",
                    v: amount,
                    min: 0.0,
                    max: 1.0,
                },
                F {
                    label: "Size",
                    v: size,
                    min: 0.5,
                    max: 8.0,
                },
                I {
                    label: "Seed",
                    v: seed,
                    min: 0,
                    max: 9999,
                },
            ],
            Op::Mirror { mode } => vec![C {
                label: "Mode",
                v: mode,
                options: MIRROR_MODES,
            }],
            Op::ChannelSwap { mode } => vec![C {
                label: "Mode",
                v: mode,
                options: SWAP_MODES,
            }],
            Op::Invert { mix } => vec![F {
                label: "Mix",
                v: mix,
                min: 0.0,
                max: 1.0,
            }],
            Op::Wave {
                amp,
                freq,
                vertical,
                phase,
            } => vec![
                F {
                    label: "Amp %",
                    v: amp,
                    min: 0.0,
                    max: 10.0,
                },
                F {
                    label: "Freq",
                    v: freq,
                    min: 0.5,
                    max: 60.0,
                },
                F {
                    label: "Phase",
                    v: phase,
                    min: 0.0,
                    max: 1.0,
                },
                B {
                    label: "Vertical",
                    v: vertical,
                },
            ],
            Op::Blend { mode, mix } => vec![
                C {
                    label: "Mode",
                    v: mode,
                    options: BLEND_MODES,
                },
                F {
                    label: "Mix",
                    v: mix,
                    min: 0.0,
                    max: 1.0,
                },
            ],
            Op::Mask {
                threshold,
                softness,
                invert,
            } => vec![
                F {
                    label: "Threshold",
                    v: threshold,
                    min: 0.0,
                    max: 1.0,
                },
                F {
                    label: "Softness",
                    v: softness,
                    min: 0.0,
                    max: 0.5,
                },
                B {
                    label: "Invert",
                    v: invert,
                },
            ],
            Op::Split { mode } => vec![C {
                label: "Split",
                v: mode,
                options: SPLIT_MODES,
            }],
        }
    }

    /// Experimental: dice roll every parameter (deterministic for a given roll number).
    pub fn randomize(&mut self, roll: u32) {
        for (i, p) in self.params().into_iter().enumerate() {
            let r = crate::img::hash01(roll, i as u32, 99);
            match p {
                Param::F { v, min, max, .. } => *v = min + (max - min) * r,
                Param::I { v, min, max, .. } => *v = min + ((max - min) as f32 * r).round() as i32,
                Param::B { v, .. } => *v = r > 0.5,
                Param::C { v, options, .. } => {
                    *v = ((options.len() as f32 * r) as usize).min(options.len() - 1)
                }
            }
        }
    }

    /// Apply to the inputs, producing every output (one for most ops, three for Split).
    pub fn apply_outputs(&self, mut inputs: Vec<Option<Arc<Img>>>) -> Option<Vec<Img>> {
        if let Op::Split { mode } = self {
            let src = inputs.get_mut(0)?.take()?;
            return Some(basic::split(&src, *mode));
        }
        self.apply(inputs).map(|i| vec![i])
    }

    /// Apply to the inputs. `None` if a required input is missing.
    pub fn apply(&self, mut inputs: Vec<Option<Arc<Img>>>) -> Option<Img> {
        let take = |a: Arc<Img>| Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone());
        if let Op::Mask {
            threshold,
            softness,
            invert,
        } = self
        {
            let a = inputs.get_mut(0)?.take()?;
            let b = inputs.get_mut(1).and_then(|b| b.take());
            let m = inputs.get_mut(2).and_then(|m| m.take());
            return Some(match b {
                Some(b) => basic::mask(take(a), &b, m.as_deref(), *threshold, *softness, *invert),
                None => take(a),
            });
        }
        if let Op::Blend { mode, mix } = self {
            let a = inputs.get_mut(0)?.take()?;
            return match inputs.get_mut(1).and_then(|b| b.take()) {
                Some(b) => Some(basic::blend(take(a), &b, *mode, *mix)),
                None => Some(take(a)),
            };
        }
        let src = take(inputs.get_mut(0)?.take()?);
        Some(match self {
            Op::Scan | Op::Output => src,
            Op::Crop(c) => basic::crop(&src, c),
            Op::WhiteBalance { temp, tint } => basic::white_balance(src, *temp, *tint),
            Op::Exposure { ev } => basic::exposure(src, *ev),
            Op::BrightnessContrast {
                brightness,
                contrast,
            } => basic::brightness_contrast(src, *brightness, *contrast),
            Op::Clarity { amount } => basic::clarity(src, *amount),
            Op::Saturation {
                saturation,
                vibrance,
            } => basic::saturation(src, *saturation, *vibrance),
            Op::RgbSplit { amount, angle } => glitch::rgb_split(&src, *amount, *angle),
            Op::PixelSort {
                lo,
                hi,
                vertical,
                reverse,
                key,
            } => glitch::pixel_sort(src, *lo, *hi, *vertical, *reverse, *key),
            Op::SliceShift {
                bands,
                shift,
                chance,
                seed,
            } => glitch::slice_shift(&src, *bands, *shift, *chance, *seed as u32),
            Op::BlockGlitch {
                block,
                chance,
                mode,
                seed,
            } => glitch::block_glitch(&src, *block, *chance, *mode, *seed as u32),
            Op::BitCrush { levels } => glitch::bit_crush(src, *levels),
            Op::Vhs {
                jitter,
                bleed,
                lines,
                noise,
                seed,
            } => glitch::vhs(&src, *jitter, *bleed, *lines, *noise, *seed as u32),
            Op::Dither {
                levels,
                scale,
                mono,
            } => glitch::dither(src, *levels, *scale, *mono),
            Op::Pixelate { cell } => glitch::pixelate(&src, *cell),
            Op::Grain { amount, size, seed } => glitch::grain(src, *amount, *size, *seed as u32),
            Op::Mirror { mode } => glitch::mirror(&src, *mode),
            Op::ChannelSwap { mode } => glitch::channel_swap(src, *mode),
            Op::Invert { mix } => glitch::invert(src, *mix),
            Op::Wave {
                amp,
                freq,
                vertical,
                phase,
            } => glitch::wave(&src, *amp, *freq, *vertical, *phase),
            Op::Blend { .. } | Op::Mask { .. } => unreachable!(),
            Op::Split { mode } => basic::split(&src, *mode).swap_remove(0),
        })
    }
}
