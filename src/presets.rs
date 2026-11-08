//! Node-sequence presets: built-in looks, built-in glitch patches and user presets stored in
//! `library/_presets/<name>.json`.

use crate::graph::{self, Graph, NodeData, chain_graph};
use crate::ops::{Aspect, Crop, Op};
use egui::pos2;
use egui_snarl::{InPinId, OutPinId};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresetKind {
    Look,
    Glitch,
    User,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub kind: PresetKind,
    pub graph: Graph,
}

fn crop() -> Op {
    Op::Crop(Crop::default())
}
fn wb() -> Op {
    Op::WhiteBalance {
        temp: 0.0,
        tint: 0.0,
    }
}

pub fn builtin() -> Vec<Preset> {
    use PresetKind::*;
    let p = |name: &str, kind, graph| Preset {
        name: name.into(),
        kind,
        graph,
    };
    vec![
        p("Neutral", Look, graph::default_graph()),
        p(
            "Clean Scan",
            Look,
            chain_graph(vec![
                crop(),
                wb(),
                Op::Exposure { ev: 0.15 },
                Op::BrightnessContrast {
                    brightness: 5.0,
                    contrast: 12.0,
                },
                Op::Clarity { amount: 30.0 },
                Op::Saturation {
                    saturation: 0.0,
                    vibrance: 10.0,
                },
            ]),
        ),
        p(
            "Crisp Object",
            Look,
            chain_graph(vec![
                crop(),
                wb(),
                Op::BrightnessContrast {
                    brightness: 0.0,
                    contrast: 18.0,
                },
                Op::Clarity { amount: 45.0 },
                Op::Saturation {
                    saturation: -5.0,
                    vibrance: 15.0,
                },
            ]),
        ),
        p(
            "Mono Archive",
            Look,
            chain_graph(vec![
                crop(),
                wb(),
                Op::Saturation {
                    saturation: -100.0,
                    vibrance: 0.0,
                },
                Op::BrightnessContrast {
                    brightness: 0.0,
                    contrast: 22.0,
                },
                Op::Clarity { amount: 35.0 },
            ]),
        ),
        p(
            "VHS Tape",
            Glitch,
            chain_graph(vec![
                crop(),
                wb(),
                Op::Vhs {
                    jitter: 0.5,
                    bleed: 0.6,
                    lines: 0.5,
                    noise: 0.2,
                    seed: 7,
                },
                Op::RgbSplit {
                    amount: 0.4,
                    angle: 0.0,
                },
                Op::Grain {
                    amount: 0.2,
                    size: 1.5,
                    seed: 3,
                },
            ]),
        ),
        p(
            "Pixel Melt",
            Glitch,
            chain_graph(vec![
                crop(),
                wb(),
                Op::PixelSort {
                    lo: 0.3,
                    hi: 0.85,
                    vertical: true,
                    reverse: false,
                    key: 0,
                },
                Op::Saturation {
                    saturation: 20.0,
                    vibrance: 0.0,
                },
            ]),
        ),
        p(
            "Datamosh",
            Glitch,
            chain_graph(vec![
                crop(),
                wb(),
                Op::BlockGlitch {
                    block: 4.0,
                    chance: 0.15,
                    mode: 3,
                    seed: 42,
                },
                Op::SliceShift {
                    bands: 18,
                    shift: 8.0,
                    chance: 0.3,
                    seed: 5,
                },
            ]),
        ),
        p(
            "RGB Ghost",
            Glitch,
            chain_graph(vec![
                crop(),
                wb(),
                Op::RgbSplit {
                    amount: 2.0,
                    angle: 30.0,
                },
                Op::Wave {
                    amp: 0.6,
                    freq: 3.0,
                    vertical: false,
                    phase: 0.0,
                },
            ]),
        ),
        p(
            "Bit Crush",
            Glitch,
            chain_graph(vec![
                crop(),
                wb(),
                Op::BitCrush { levels: 5 },
                Op::Dither {
                    levels: 4,
                    scale: 2,
                    mono: false,
                },
            ]),
        ),
        p(
            "Scan Drift",
            Glitch,
            chain_graph(vec![
                crop(),
                wb(),
                Op::SliceShift {
                    bands: 120,
                    shift: 2.5,
                    chance: 0.6,
                    seed: 9,
                },
                Op::Vhs {
                    jitter: 0.2,
                    bleed: 0.3,
                    lines: 0.8,
                    noise: 0.1,
                    seed: 2,
                },
            ]),
        ),
        p(
            "Kaleido",
            Glitch,
            chain_graph(vec![
                Op::Crop(Crop {
                    aspect: Aspect::Square,
                    ..Default::default()
                }),
                wb(),
                Op::Mirror { mode: 4 },
                Op::Clarity { amount: 30.0 },
            ]),
        ),
        p(
            "Thermal",
            Glitch,
            chain_graph(vec![
                crop(),
                Op::ChannelSwap { mode: 2 },
                Op::Invert { mix: 1.0 },
                Op::BitCrush { levels: 8 },
                Op::Saturation {
                    saturation: 60.0,
                    vibrance: 0.0,
                },
            ]),
        ),
        p("Double Exposure", Glitch, double_exposure()),
        p("Channel Drift", Glitch, channel_drift()),
        p("Glitch Highlights", Glitch, glitch_highlights()),
    ]
}

/// A branching patch: the cropped scan is blended (Screen) with a mirrored, half-inverted copy.
fn double_exposure() -> Graph {
    let mut g = Graph::new();
    let scan = g.insert_node(pos2(0.0, 0.0), NodeData::new(Op::Scan));
    let crop = g.insert_node(pos2(290.0, 0.0), NodeData::new(crop()));
    let mirror = g.insert_node(pos2(580.0, 170.0), NodeData::new(Op::Mirror { mode: 1 }));
    let inv = g.insert_node(pos2(870.0, 170.0), NodeData::new(Op::Invert { mix: 0.6 }));
    let blend = g.insert_node(
        pos2(1160.0, 0.0),
        NodeData::new(Op::Blend { mode: 2, mix: 0.7 }),
    );
    let out = g.insert_node(pos2(1450.0, 0.0), NodeData::new(Op::Output));
    let w = |g: &mut Graph, a, b, i| {
        g.connect(
            OutPinId { node: a, output: 0 },
            InPinId { node: b, input: i },
        );
    };
    w(&mut g, scan, crop, 0);
    w(&mut g, crop, blend, 0);
    w(&mut g, crop, mirror, 0);
    w(&mut g, mirror, inv, 0);
    w(&mut g, inv, blend, 1);
    w(&mut g, blend, out, 0);
    g
}

fn wire(g: &mut Graph, a: egui_snarl::NodeId, out: usize, b: egui_snarl::NodeId, input: usize) {
    g.connect(
        OutPinId {
            node: a,
            output: out,
        },
        InPinId { node: b, input },
    );
}

/// Split R / G / B, push red and blue around differently, add the channels back together.
fn channel_drift() -> Graph {
    let mut g = Graph::new();
    let n = |g: &mut Graph, x: f32, y: f32, op: Op| g.insert_node(pos2(x, y), NodeData::new(op));
    let scan = n(&mut g, 0.0, 0.0, Op::Scan);
    let crop = n(&mut g, 290.0, 0.0, crop());
    let split = n(&mut g, 580.0, 0.0, Op::Split { mode: 0 });
    let red = n(
        &mut g,
        870.0,
        -190.0,
        Op::SliceShift {
            bands: 30,
            shift: 9.0,
            chance: 0.45,
            seed: 3,
        },
    );
    let blue = n(
        &mut g,
        870.0,
        190.0,
        Op::Wave {
            amp: 1.2,
            freq: 9.0,
            vertical: false,
            phase: 0.3,
        },
    );
    let add1 = n(&mut g, 1160.0, -60.0, Op::Blend { mode: 5, mix: 1.0 });
    let add2 = n(&mut g, 1450.0, 0.0, Op::Blend { mode: 5, mix: 1.0 });
    let out = n(&mut g, 1740.0, 0.0, Op::Output);
    wire(&mut g, scan, 0, crop, 0);
    wire(&mut g, crop, 0, split, 0);
    wire(&mut g, split, 0, red, 0);
    wire(&mut g, split, 2, blue, 0);
    wire(&mut g, red, 0, add1, 0);
    wire(&mut g, split, 1, add1, 1);
    wire(&mut g, add1, 0, add2, 0);
    wire(&mut g, blue, 0, add2, 1);
    wire(&mut g, add2, 0, out, 0);
    g
}

/// Block-glitch only the bright parts of the object (Mask keyed on the clean image).
fn glitch_highlights() -> Graph {
    let mut g = Graph::new();
    let n = |g: &mut Graph, x: f32, y: f32, op: Op| g.insert_node(pos2(x, y), NodeData::new(op));
    let scan = n(&mut g, 0.0, 0.0, Op::Scan);
    let crop = n(&mut g, 290.0, 0.0, crop());
    let wbn = n(&mut g, 580.0, 0.0, wb());
    let glitch = n(
        &mut g,
        870.0,
        -190.0,
        Op::BlockGlitch {
            block: 4.0,
            chance: 0.5,
            mode: 3,
            seed: 7,
        },
    );
    let mask = n(
        &mut g,
        1160.0,
        0.0,
        Op::Mask {
            threshold: 0.62,
            softness: 0.12,
            invert: false,
        },
    );
    let out = n(&mut g, 1450.0, 0.0, Op::Output);
    wire(&mut g, scan, 0, crop, 0);
    wire(&mut g, crop, 0, wbn, 0);
    wire(&mut g, wbn, 0, glitch, 0);
    wire(&mut g, glitch, 0, mask, 0);
    wire(&mut g, wbn, 0, mask, 1);
    wire(&mut g, mask, 0, out, 0);
    g
}

/// Apply a preset to the current graph. Per-photo corrections are kept: the crop (framing)
/// and white balance (scanner-specific). A preset asking for a specific aspect (Kaleido = 1:1)
/// still sets that aspect.
pub fn apply(current: &Graph, preset: &Graph) -> Graph {
    let mut g = preset.clone();
    let cur_crop = graph::crop_node(current).map(|id| current[id].clone());
    if let Some(mut cc) = cur_crop {
        match graph::crop_node(&g) {
            Some(pc) => {
                let preset_aspect = match &g[pc].op {
                    Op::Crop(c) => c.aspect,
                    _ => Aspect::Free,
                };
                if preset_aspect != Aspect::Free
                    && let Op::Crop(c) = &mut cc.op
                {
                    c.aspect = preset_aspect;
                    c.rect = [0.0, 0.0, 1.0, 1.0];
                }
                g[pc] = cc;
            }
            None => {
                if let Some(scan) = graph::scan_node(&g) {
                    let id = graph::insert_after(&mut g, scan, Op::Output);
                    g[id] = cc;
                }
            }
        }
    }
    let is_wb = |o: &Op| matches!(o, Op::WhiteBalance { .. });
    let cur_wb = graph::find(current, is_wb)
        .map(|id| current[id].clone())
        .filter(|n| n.op != wb());
    if let Some(cw) = cur_wb {
        match graph::find(&g, is_wb) {
            Some(pw) => g[pw] = cw,
            None => {
                let after = graph::crop_node(&g).or_else(|| graph::scan_node(&g));
                if let Some(a) = after {
                    let id = graph::insert_after(&mut g, a, Op::Output);
                    g[id] = cw;
                }
            }
        }
    }
    g
}

pub fn load_user(dir: &Path) -> Vec<Preset> {
    let mut v: Vec<Preset> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            serde_json::from_str::<Preset>(&std::fs::read_to_string(e.path()).ok()?).ok()
        })
        .map(|mut p| {
            p.kind = PresetKind::User;
            p
        })
        .collect();
    v.sort_by_key(|a| a.name.to_lowercase());
    v
}

pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn save_user(dir: &Path, name: &str, graph: &Graph) -> anyhow::Result<Preset> {
    std::fs::create_dir_all(dir)?;
    let p = Preset {
        name: name.trim().to_string(),
        kind: PresetKind::User,
        graph: graph.clone(),
    };
    let file = dir.join(format!("{}.json", sanitize_name(name)));
    std::fs::write(file, serde_json::to_string_pretty(&p)?)?;
    Ok(p)
}

pub fn delete_user(dir: &Path, name: &str) -> anyhow::Result<()> {
    std::fs::remove_file(dir.join(format!("{}.json", sanitize_name(name))))?;
    Ok(())
}
