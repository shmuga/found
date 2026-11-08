//! Edit graph (egui-snarl is the data model) + evaluation with a per-node result cache.

use crate::img::Img;
use crate::ops::{Category, Op};
use egui::pos2;
use egui_snarl::{InPinId, NodeId, OutPinId, Snarl};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NodeData {
    pub op: Op,
    #[serde(default)]
    pub bypass: bool,
}

impl NodeData {
    pub fn new(op: Op) -> Self {
        Self { op, bypass: false }
    }
}

pub type Graph = Snarl<NodeData>;

const DX: f32 = 290.0;

/// Builds Scan → ops… → Output as a straight chain.
pub fn chain_graph(ops: Vec<Op>) -> Graph {
    let mut g = Graph::new();
    let mut prev = g.insert_node(pos2(0.0, 0.0), NodeData::new(Op::Scan));
    for (i, op) in ops.into_iter().enumerate() {
        let id = g.insert_node(pos2(DX * (i + 1) as f32, 0.0), NodeData::new(op));
        g.connect(
            OutPinId {
                node: prev,
                output: 0,
            },
            InPinId { node: id, input: 0 },
        );
        prev = id;
    }
    let n = g.nodes().count();
    let out = g.insert_node(pos2(DX * n as f32, 0.0), NodeData::new(Op::Output));
    g.connect(
        OutPinId {
            node: prev,
            output: 0,
        },
        InPinId {
            node: out,
            input: 0,
        },
    );
    g
}

pub fn default_graph() -> Graph {
    chain_graph(vec![
        Op::Crop(Default::default()),
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
    ])
}

pub fn find(g: &Graph, pred: impl Fn(&Op) -> bool) -> Option<NodeId> {
    g.node_ids().find(|(_, n)| pred(&n.op)).map(|(id, _)| id)
}

pub fn output_node(g: &Graph) -> Option<NodeId> {
    find(g, |o| matches!(o, Op::Output))
}

pub fn scan_node(g: &Graph) -> Option<NodeId> {
    find(g, |o| matches!(o, Op::Scan))
}

pub fn crop_node(g: &Graph) -> Option<NodeId> {
    find(g, |o| matches!(o, Op::Crop(_)))
}

/// The (node, output) wired into `node`'s input `input`.
pub fn input_pin(g: &Graph, node: NodeId, input: usize) -> Option<(NodeId, usize)> {
    g.in_pin(InPinId { node, input })
        .remotes
        .first()
        .map(|o| (o.node, o.output))
}

pub fn input_of(g: &Graph, node: NodeId, input: usize) -> Option<NodeId> {
    g.in_pin(InPinId { node, input })
        .remotes
        .first()
        .map(|o| o.node)
}

pub fn consumers(g: &Graph, node: NodeId) -> Vec<InPinId> {
    g.out_pin(OutPinId { node, output: 0 }).remotes
}

/// The main chain: walk upstream from Output along input 0, returned source-first.
pub fn chain(g: &Graph) -> Vec<NodeId> {
    let mut out = vec![];
    let Some(mut cur) = output_node(g) else {
        return out;
    };
    let mut guard = 0;
    loop {
        out.push(cur);
        guard += 1;
        match input_of(g, cur, 0) {
            Some(prev) if guard < 512 && !out.contains(&prev) => cur = prev,
            _ => break,
        }
    }
    out.reverse();
    out
}

/// Nodes not on the main chain (branches, Blend B inputs, loose nodes).
pub fn off_chain(g: &Graph) -> Vec<NodeId> {
    let c = chain(g);
    g.node_ids()
        .map(|(id, _)| id)
        .filter(|id| !c.contains(id))
        .collect()
}

/// Insert `op` into the chain right after `after` (between it and its consumers on the chain).
pub fn insert_after(g: &mut Graph, after: NodeId, op: Op) -> NodeId {
    let pos = g.get_node_info(after).map(|n| n.pos).unwrap_or_default();
    let id = g.insert_node(pos2(pos.x + DX * 0.5, pos.y + 120.0), NodeData::new(op));
    let downstream: Vec<InPinId> = consumers(g, after);
    for to in &downstream {
        g.disconnect(
            OutPinId {
                node: after,
                output: 0,
            },
            *to,
        );
        g.connect(
            OutPinId {
                node: id,
                output: 0,
            },
            *to,
        );
    }
    g.connect(
        OutPinId {
            node: after,
            output: 0,
        },
        InPinId { node: id, input: 0 },
    );
    relayout_chain(g);
    id
}

/// Remove a node, bridging its input-0 source to its consumers.
pub fn remove_bridging(g: &mut Graph, node: NodeId) {
    let src = input_of(g, node, 0);
    let downstream = consumers(g, node);
    g.remove_node(node);
    if let Some(s) = src {
        for to in downstream {
            g.connect(OutPinId { node: s, output: 0 }, to);
        }
    }
    relayout_chain(g);
}

/// Swap a chain node with its upstream (dir = -1) or downstream (dir = +1) neighbour.
pub fn move_in_chain(g: &mut Graph, node: NodeId, dir: i32) {
    let c = chain(g);
    let Some(i) = c.iter().position(|&n| n == node) else {
        return;
    };
    let j = i as i32 + dir;
    if j < 1 || j as usize >= c.len() - 1 || i == 0 || i == c.len() - 1 {
        return; // never move past Scan/Output
    }
    let (a, b) = if dir < 0 {
        (c[j as usize], node)
    } else {
        (node, c[j as usize])
    };
    // a → b becomes b → a. upstream(a) feeds b; b feeds a; a feeds downstream(b) on input 0.
    let up = input_of(g, a, 0);
    let down: Vec<InPinId> = consumers(g, b);
    if let Some(u) = up {
        g.disconnect(
            OutPinId { node: u, output: 0 },
            InPinId { node: a, input: 0 },
        );
    }
    g.disconnect(
        OutPinId { node: a, output: 0 },
        InPinId { node: b, input: 0 },
    );
    for d in &down {
        g.disconnect(OutPinId { node: b, output: 0 }, *d);
    }
    if let Some(u) = up {
        g.connect(
            OutPinId { node: u, output: 0 },
            InPinId { node: b, input: 0 },
        );
    }
    g.connect(
        OutPinId { node: b, output: 0 },
        InPinId { node: a, input: 0 },
    );
    for d in down {
        g.connect(OutPinId { node: a, output: 0 }, d);
    }
    relayout_chain(g);
}

/// Lay the main chain out left-to-right so the patch stays readable after presentation edits.
pub fn relayout_chain(g: &mut Graph) {
    for (i, id) in chain(g).into_iter().enumerate() {
        if let Some(n) = g.get_node_info_mut(id) {
            n.pos = pos2(DX * i as f32, 0.0);
        }
    }
}

/// Would connecting from → to create a cycle?
pub fn would_cycle(g: &Graph, from: NodeId, to: NodeId) -> bool {
    use petgraph::graphmap::DiGraphMap;
    let mut pg = DiGraphMap::<usize, ()>::new();
    for (id, _) in g.node_ids() {
        pg.add_node(id.0);
    }
    for (o, i) in g.wires() {
        pg.add_edge(o.node.0, i.node.0, ());
    }
    pg.add_edge(from.0, to.0, ());
    petgraph::algo::is_cyclic_directed(&pg)
}

pub fn has_experimental(g: &Graph) -> bool {
    g.nodes().any(|n| n.op.category() == Category::Experimental)
}

/// Stable content key of a graph (for undo/dirty checks).
pub fn graph_json(g: &Graph) -> String {
    serde_json::to_string(g).unwrap_or_default()
}

// ------------------------------------------------------------------ evaluation

pub struct Evaluator {
    /// node → (key, result). Only used when caching is enabled (preview).
    /// (node, output) → (key, result).
    cache: HashMap<(NodeId, usize), (u64, Arc<Img>)>,
    caching: bool,
    /// Uncached mode: the source plus how many Scan visits remain, so the last visit can hand
    /// over the only reference (no 50MP copy).
    once: Option<(Arc<Img>, usize)>,
}

impl Evaluator {
    pub fn new(caching: bool) -> Self {
        Self {
            cache: HashMap::new(),
            caching,
            once: None,
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    fn node_key(g: &Graph, node: NodeId, src_key: u64, depth: usize) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if depth > 256 {
            return 0;
        }
        let Some(n) = g.get_node(node) else { return 0 };
        serde_json::to_string(n).unwrap_or_default().hash(&mut h);
        if matches!(n.op, Op::Scan) {
            src_key.hash(&mut h);
        }
        for i in 0..n.op.inputs() {
            match input_pin(g, node, i) {
                Some((up, out)) => (Self::node_key(g, up, src_key, depth + 1), out).hash(&mut h),
                None => 0u64.hash(&mut h),
            }
        }
        h.finish()
    }

    /// Evaluate `node`'s output.
    pub fn eval(
        &mut self,
        g: &Graph,
        src: &Arc<Img>,
        src_key: u64,
        node: NodeId,
    ) -> Option<Arc<Img>> {
        self.eval_inner(g, src, src_key, node, 0, 0)
    }

    /// Evaluate output `out` of `node` (only Split has more than one).
    fn eval_inner(
        &mut self,
        g: &Graph,
        src: &Arc<Img>,
        src_key: u64,
        node: NodeId,
        out: usize,
        depth: usize,
    ) -> Option<Arc<Img>> {
        if depth > 256 {
            return None; // cycle guard
        }
        let n = g.get_node(node)?;
        if matches!(n.op, Op::Scan) {
            if let Some((_, left)) = &mut self.once {
                *left -= 1;
                if *left == 0 {
                    return self.once.take().map(|(a, _)| a);
                }
                return self.once.as_ref().map(|(a, _)| a.clone());
            }
            return Some(src.clone());
        }
        let key = if self.caching {
            Self::node_key(g, node, src_key, 0)
        } else {
            0
        };
        if self.caching
            && let Some((k, img)) = self.cache.get(&(node, out))
            && *k == key
        {
            return Some(img.clone());
        }
        let n_in = if n.bypass || matches!(n.op, Op::Output) {
            1
        } else {
            n.op.inputs()
        };
        let mut inputs = Vec::with_capacity(n_in);
        for i in 0..n_in {
            inputs.push(
                input_pin(g, node, i)
                    .and_then(|(up, o)| self.eval_inner(g, src, src_key, up, o, depth + 1)),
            );
        }
        if n.bypass || matches!(n.op, Op::Output) {
            // Bypassed: every output passes the input through.
            let img = inputs.into_iter().next().flatten()?;
            if self.caching {
                self.cache.insert((node, out), (key, img.clone()));
            }
            return Some(img);
        }
        let mut outs: Vec<Arc<Img>> =
            n.op.apply_outputs(inputs)?
                .into_iter()
                .map(Arc::new)
                .collect();
        if out >= outs.len() {
            return None;
        }
        if self.caching {
            for (k, img) in outs.iter().enumerate() {
                self.cache.insert((node, k), (key, img.clone()));
            }
        }
        Some(outs.swap_remove(out))
    }

    pub fn eval_output(&mut self, g: &Graph, src: &Arc<Img>, src_key: u64) -> Option<Arc<Img>> {
        let out = output_node(g)?;
        self.eval(g, src, src_key, out)
    }

    /// Input image of a node (for crop editing and the eyedropper).
    pub fn eval_input(
        &mut self,
        g: &Graph,
        src: &Arc<Img>,
        src_key: u64,
        node: NodeId,
    ) -> Option<Arc<Img>> {
        let up = input_of(g, node, 0)?;
        self.eval(g, src, src_key, up)
    }

    /// Drop cache entries for nodes no longer in the graph.
    pub fn prune(&mut self, g: &Graph) {
        self.cache.retain(|(id, _), _| g.get_node(*id).is_some());
    }
}

/// Full-resolution, uncached render (memory: only the live chain is held).
pub fn render_full(g: &Graph, src: Img) -> Option<Img> {
    let out_node = output_node(g)?;
    let uses = count_scan_visits(g, out_node, 0);
    let mut ev = Evaluator::new(false);
    let dummy = Arc::new(Img::new(1, 1));
    ev.once = Some((Arc::new(src), uses.max(1)));
    let out = ev.eval(g, &dummy, 0, out_node)?;
    Some(Arc::try_unwrap(out).unwrap_or_else(|a| (*a).clone()))
}

/// How many times an uncached evaluation will reach the Scan node.
fn count_scan_visits(g: &Graph, node: NodeId, depth: usize) -> usize {
    if depth > 256 {
        return 0;
    }
    let Some(n) = g.get_node(node) else { return 0 };
    if matches!(n.op, Op::Scan) {
        return 1;
    }
    let ins = if n.bypass || matches!(n.op, Op::Output) {
        1
    } else {
        n.op.inputs()
    };
    (0..ins)
        .filter_map(|i| input_of(g, node, i))
        .map(|up| count_scan_visits(g, up, depth + 1))
        .sum()
}
