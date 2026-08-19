# Diagrams

Design docs use **rendered diagrams**, not ASCII art. Two formats, chosen by fit:

## 1. Mermaid — for flowcharts / sequences / simple graphs
Write a ```mermaid fenced block directly in the Markdown. It is text (diffable),
renders natively on GitHub and most Markdown viewers, and needs no build step.
Prefer this for data-flow, state, and dependency diagrams.

## 2. SVG (via domotion) — for designed, layout-rich diagrams
For diagrams where layout/typography matter (the architecture overview, etc.), we
author an **HTML** file and render it to a **self-contained SVG** with
[`domotion`](https://www.npmjs.com/package/domotion-svg) (installed globally). The
SVG embeds in Markdown with a normal image link and scales crisply (text is emitted
as glyph paths, so it looks identical everywhere).

- **Sources** live in [`src/*.html`](src/) — edit these to change a diagram.
- **Rendered output** is the sibling `*.svg`, referenced from docs via
  `![alt](diagrams/foo.svg)`.
- Give the HTML an explicit **light card background** so the SVG is readable on any
  Markdown background (GitHub light *or* dark).

### Regenerate an SVG
```sh
cd docs/diagrams
domotion capture ./src/architecture.html -o architecture.svg \
  --width 1120 --height 700 --selector ".wrap"
```
`domotion` drives headless Chromium; if a sandbox blocks Chromium's Mach-port
setup (`bootstrap_check_in … Permission denied`), run it outside the sandbox.

### Preview an SVG as PNG (to eyeball it)
```sh
svg-to-image architecture.svg -o /tmp/preview.png --width 1120 --background '#f6f7f9'
```

## Files
| File | Used by |
|---|---|
| `architecture.svg` (`src/architecture.html`) | [`01-architecture.md`](../01-architecture.md) §1.1 |
