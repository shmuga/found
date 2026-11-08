# Found — scans of found objects

## Stack
- Rust 1.95 (pinned in rust-toolchain.toml), `eframe`/`egui` 0.36 on wgpu, `egui-snarl` 0.12 (node editor),
  `petgraph` (graph eval/toposort), `image` (decode/encode), `rfd` (dialogs), `serde`/`serde_json`.
- Tests: `egui_kittest` (headless UI e2e, AccessKit-driven, snapshot images) + unit tests for image ops.
- Algorithms reference: https://github.com/storytold/photocraft

## Inputs
- Large JPEG scans (>50MP). Edit on a 2400px proxy (JPEG DCT-scaled decode, ~80 ms); render full-res on export.
- Image ops run on the CPU (rayon, f32), with a per-node result cache so a slider only re-runs downstream
  nodes (5–35 ms per change on the proxy; ~0.6 s full 50MP render + encode). wgpu is used for display;
  moving ops to compute shaders is a possible later optimisation.
- Import: drag & drop / file picker, plus an `Inbox/` folder the user copies scans into.

## Settings (no hardcoded folders)
- JSON in the OS config dir (override with `SCANNER_SETTINGS`). Startup picker with recent libraries
  unless "open last library on startup" is enabled; CLI arg opens a library directly.
- Per-library folder layout (Inbox / collages / presets; relative or absolute), JPEG quality,
  preview size, thumbnail cache folder. Folder names below are defaults only.

## Library on disk (folder per collection)
```
library/
  Inbox/                 # user drops scans here
  Coins/
    0001.orig.jpg        # original, moved from Inbox, never modified
    0001.edit.jpg        # rendered export, JPEG q95 full-res
    0001.json            # sidecar: graph recipe (non-destructive), tags, metadata
  Stamps/ ...
  _collages/
    collage-0001.json    # layers, masks, template, background
    collage-0001.jpg
```
- Filing from Inbox: move + sequential name within the collection.
- A photo belongs to exactly one collection; tags (in sidecar) cross-cut. Tag filter across all collections.

## Editing — patch + presentation
- Each photo has an edit graph: `Scan → Crop → WB → Exposure → Brightness/Contrast → Clarity → Out`.
- Presentation view (default): preview + slider panel. Toggle to Patch view to rewire / add / bypass nodes.
- Crop: 1:1, 3:2, 2:3 (orientation swap), freehand; drag handles on preview.
- WB: Temp + Tint sliders, eyedropper (click neutral area), Auto (gray-world / white-patch).
- Exposure (stops), Brightness, Contrast, Clarity (local midtone contrast, Fuji-style).
- Non-destructive: recipe stored in sidecar; re-editable any time.

## Node sequences, presets, experimental
- The chain is customizable in both views: add (Basic / Experimental), reorder, bypass, remove; branch +
  Blend in Patch.
- Presets: built-in looks (Neutral, Clean Scan, Crisp Object, Mono Archive) and glitch patches (VHS Tape,
  Pixel Melt, Datamosh, RGB Ghost, Bit Crush, Scan Drift, Kaleido, Thermal, Double Exposure). User presets in
  `library/_presets/`. Applying keeps the photo's crop + white balance.
- Experimental nodes: RGB Split, Pixel Sort, Slice Shift, Block Glitch, Bit Crush, VHS, Dither, Pixelate,
  Grain, Mirror/Kaleido, Channel Swap, Invert, Wave, Blend. Per-node Randomize + Mutate-all. Seeded and
  resolution-independent.

## Collage — separate workspace
- Canvas presets (1:1, 3:2, 2:3, A4 @ DPI, custom), background color.
- Split-frame templates (2-up, 3-up, 2×2…) with gutter; or freehand placement.
- Layers panel: order, visibility, lock. Drag / scale / rotate.
- Per-layer non-destructive mask: rect crop, freehand polygon, lasso cutout.
- Export JPEG q95 full-res.

## Look
- Light-grey theme: mid-grey chrome, dark rounded "number box" controls, single blue accent,
  compact left sidebar (collections / Inbox / tags), snappy.

## Scope v1
All three features at MVP depth + Inbox → collection filing, tags, kittest e2e for the core loop
(import → crop → adjust → file into collection → files on disk; collage build → export).
