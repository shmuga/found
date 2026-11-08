#!/usr/bin/env python3
"""Draw docs/architecture.svg in the app's patch style: grey canvas, dark object boxes with a
coloured inlet bar (blue = I/O, green = core, yellow = processing), light bodies, wires."""

CANVAS, BOX, BODY, TEXT, LIGHT, DIM = "#c4c7cc", "#585a5e", "#dcdee2", "#16171a", "#eeeeee", "#3a3c40"
IO, CORE, FX = "#609cec", "#54aa48", "#e2c234"
W, H = 1290, 590
BW, HH = 200, 26  # box width, header height
C = [40, 290, 540, 790, 1040]  # column x
R = [50, 230, 410]             # row y

nodes = {
    # id: (x, y, colour, title, body lines)
    "scans":   (C[0], R[0], IO, "Scans", ["JPEG · PNG · TIFF · PDF", "scanner output / drop"]),
    "io":      (C[1], R[0], IO, "io · pdf", ["DCT-scaled decode", "PDF page extraction"]),
    "lib":     (C[2], R[0], CORE, "Library", ["Inbox · collections", "sidecars · local trash"]),
    "ui":      (C[3], R[0], CORE, "UI (egui)", ["grid · editor · collage", "vim keys · ⌘ shortcuts"]),
    "graph":   (C[4], R[0], FX, "Recipe graph", ["Scan → ops → Output", "Blend · Mask · Split"]),
    "settings":(C[1], R[1], IO, "Settings", ["JSON in config dir", "folders · layout · keys"]),
    "collage": (C[2], R[1], CORE, "Collage", ["layers · masks · frames", "→ collage.jpg"]),
    "worker":  (C[3], R[1], CORE, "Worker", ["background jobs", "rayon pool"]),
    "eval":    (C[4], R[1], FX, "Evaluator", ["per-output cache", "live preview"]),
    "disk":    (C[2], R[2], IO, "Library folder", ["0001.orig.jpg · .json", "0001.edit.jpg · .found/"]),
    "full":    (C[3], R[2], FX, "render_full", ["full-res, uncached", "→ edit.jpg (q95)"]),
    "ops":     (C[4], R[2], FX, "ops", ["crop · WB · clarity", "glitch · blend · mask · split"]),
}

def box_h(n):
    return HH + 14 + 16 * len(nodes[n][4])

def right(n, dy=HH / 2):
    x, y, *_ = nodes[n]
    return (x + BW, y + dy)

def left(n, dy=HH / 2):
    x, y, *_ = nodes[n]
    return (x, y + dy)

def bottom(n, dx=BW / 2):
    x, y, *_ = nodes[n]
    return (x + dx, y + box_h(n))

def top(n, dx=BW / 2):
    x, y, *_ = nodes[n]
    return (x + dx, y)

# wires: polylines (routed by hand, axis-aligned) + colour
wires = [
    ([right("scans"), left("io")], IO),
    ([right("io"), left("lib")], IO),
    ([right("lib"), left("ui")], CORE),
    ([right("ui"), left("graph")], CORE),
    ([bottom("graph"), top("eval")], FX),
    ([bottom("eval"), top("ops")], FX),
    ([left("eval"), right("worker")], FX),
    ([top("worker"), bottom("ui")], CORE),
    ([bottom("worker"), top("full")], FX),
    ([left("full"), right("disk")], FX),
    ([bottom("lib"), top("collage")], CORE),
    ([bottom("collage"), top("disk")], CORE),
    ([right("settings"), (C[2] - 25, R[1] + HH / 2), (C[2] - 25, R[0] + 52), left("lib", 52)], IO),
]

def path(pts, r=10):
    """Polyline with rounded corners (the patch view's axis-aligned wires)."""
    d = f"M{pts[0][0]},{pts[0][1]}"
    for i in range(1, len(pts) - 1):
        (ax, ay), (bx, by), (cx, cy) = pts[i - 1], pts[i], pts[i + 1]
        ux, uy = (bx - ax), (by - ay)
        vx, vy = (cx - bx), (cy - by)
        lu, lv = max(abs(ux), abs(uy)), max(abs(vx), abs(vy))
        rr = min(r, lu / 2, lv / 2)
        p1 = (bx - ux / lu * rr, by - uy / lu * rr)
        p2 = (bx + vx / lv * rr, by + vy / lv * rr)
        d += f" L{p1[0]},{p1[1]} Q{bx},{by} {p2[0]},{p2[1]}"
    d += f" L{pts[-1][0]},{pts[-1][1]}"
    return d

out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
       f'font-family="-apple-system, BlinkMacSystemFont, Segoe UI, Helvetica, Arial, sans-serif">',
       f'<rect width="{W}" height="{H}" rx="12" fill="{CANVAS}"/>']
for pts, c in wires:
    out.append(f'<path d="{path(pts)}" fill="none" stroke="{c}" stroke-width="2.5" stroke-linecap="round"/>')
ends = set()
for pts, c in wires:
    for p in (pts[0], pts[-1]):
        ends.add((p, c))
for n, (x, y, c, title, lines) in nodes.items():
    h = box_h(n)
    out.append(f'<rect x="{x+2}" y="{y+3}" width="{BW}" height="{h}" rx="6" fill="#000" opacity="0.12"/>')
    out.append(f'<rect x="{x}" y="{y}" width="{BW}" height="{h}" rx="6" fill="{BODY}" stroke="#8a8d92"/>')
    out.append(f'<path d="M{x},{y+HH} V{y+6} Q{x},{y} {x+6},{y} H{x+BW-6} Q{x+BW},{y} {x+BW},{y+6} V{y+HH} Z" fill="{BOX}"/>')
    out.append(f'<path d="M{x},{y+4} V{y+6} Q{x},{y} {x+6},{y} H{x+BW-6} Q{x+BW},{y} {x+BW},{y+6} V{y+4} Z" fill="{c}"/>')
    out.append(f'<text x="{x+10}" y="{y+18}" font-size="13" fill="{LIGHT}" font-weight="600">{title}</text>')
    for i, l in enumerate(lines):
        out.append(f'<text x="{x+10}" y="{y+HH+18+16*i}" font-size="11.5" fill="{DIM}">{l}</text>')
for (px, py), c in ends:
    out.append(f'<circle cx="{px}" cy="{py}" r="4" fill="{c}" stroke="{BOX}" stroke-width="1.2"/>')
legend = [(IO, "files / I/O"), (CORE, "app core"), (FX, "image processing")]
for i, (c, l) in enumerate(legend):
    x = 40 + i * 150
    out.append(f'<rect x="{x}" y="{H-34}" width="22" height="4" rx="2" fill="{c}"/>')
    out.append(f'<text x="{x+30}" y="{H-28}" font-size="11.5" fill="{TEXT}">{l}</text>')
out.append("</svg>")
open("docs/architecture.svg", "w").write("\n".join(out))
print("docs/architecture.svg")
