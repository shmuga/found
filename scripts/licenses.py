#!/usr/bin/env python3
"""Write THIRD_PARTY_LICENSES.md: the fonts egui embeds (with their full licence texts) and
every crate in the build with its licence. Run from the repo root: python3 scripts/licenses.py
"""
import glob, json, os, subprocess, collections

meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"]))
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
pkgs = {p["id"]: p for p in meta["packages"]}
root = next(p["id"] for p in meta["packages"] if p["name"] == "found")

# Normal (non-dev, non-build) dependency closure of the app = what ships in the binary.
seen, stack = set(), [root]
while stack:
    i = stack.pop()
    for d in nodes[i]["deps"]:
        if any(k.get("kind") in (None, "normal") for k in d["dep_kinds"]) and d["pkg"] not in seen:
            seen.add(d["pkg"])
            stack.append(d["pkg"])

fonts_pkg = next(pkgs[i] for i in seen if pkgs[i]["name"] == "epaint_default_fonts")
fdir = os.path.join(os.path.dirname(fonts_pkg["manifest_path"]), "fonts")

out = ["# Third-party licences", "",
       "Found is built from the open-source components below. The app's own code, theme and",
       "icons are original.", "",
       "## Fonts (embedded by egui via `epaint_default_fonts`)", "",
       "| Font | Licence |", "|---|---|",
       "| Ubuntu Light | Ubuntu Font Licence 1.0 |",
       "| Hack Regular | MIT + Bitstream Vera licence |",
       "| Noto Emoji Regular | SIL Open Font Licence 1.1 |",
       "| emoji-icon-font | MIT (glyphs from Font Awesome, SIL OFL 1.1) |", ""]
for f, title in [("UFL.txt", "Ubuntu Font Licence 1.0"), ("Hack-Regular.txt", "Hack"),
                 ("OFL.txt", "SIL Open Font Licence 1.1 (Noto Emoji)"),
                 ("emoji-icon-font-mit-license.txt", "emoji-icon-font")]:
    p = os.path.join(fdir, f)
    if os.path.exists(p):
        out += [f"### {title}", "", "```", open(p, encoding="utf-8", errors="replace").read().strip(), "```", ""]

out += ["## Rust crates", "",
        "Dual-licensed crates are used under their permissive option (MIT / Apache-2.0).",
        "`option-ext` is MPL-2.0 and used unmodified; its source is at https://crates.io/crates/option-ext.", "",
        "| Crate | Version | Licence |", "|---|---|---|"]
for i in sorted(seen, key=lambda i: pkgs[i]["name"]):
    p = pkgs[i]
    out.append(f"| {p['name']} | {p['version']} | {p.get('license') or 'see crate'} |")
out.append("")
open("THIRD_PARTY_LICENSES.md", "w").write("\n".join(out))
by = collections.Counter(pkgs[i].get("license") for i in seen)
print(f"THIRD_PARTY_LICENSES.md: {len(seen)} crates; GPL-only: {[k for k in by if k and 'GPL' in k and 'OR' not in k]}")
