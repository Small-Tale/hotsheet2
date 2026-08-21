---
id: 01M0H6M3SJCYFJFHH6W0MGTV9Z
slug: HS2-PNMSBP
title: 'DECIDE (area 12): Settings & sharing layers — simplify the layer model?'
category: investigation
priority: default
status: started
created_at: 2026-08-19T01:30:44.611Z
updated_at: 2026-08-19T05:21:43.830Z
legacy_number: HS2-34
schema: 1
---

Recommend: keep-with-changes. Shared|Local|Resolved scope + classification maps onto the shared-vs-local data model (docs/02 §2.11, HS2-21). Decide: can the layered-settings UI be simplified? See docs/11 area 12. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SSDVX0KKJN3MPJG55N -->
2026-08-19T05:18:10.504Z — i would love to simplify the settings ui, but i dont have any great ideas yet.  lets iterate on this together

<!-- note: 01M0H6M3SS00V1F0NJM96HD66G -->
2026-08-19T05:21:43.830Z — FEEDBACK NEEDED: Settings UI simplification — here's my read + some directions to react to.

**Why HS1's feels heavy:** it makes the *mechanism* the interface — the Shared / Local-overrides / Resolved segmented control, per-key scope routing, override/reset affordances, and ~13 tabs. You see the plumbing before the settings.

**Directions (pick what resonates, add your own):**
1. **Effective-first.** Default view shows the resolved value; the shared-vs-local distinction only surfaces when a value is *actually* overridden.
2. **Scope as an inline per-setting detail, not a dialog-wide mode.** Each control shows a tiny badge (shared / this device / global) + an inline "override here" action — instead of switching the whole dialog into a layer.
3. **Mostly-automatic routing.** Each setting has an obvious default scope; the UI just does the right thing and shows what it did, rather than asking.
4. **Fewer tabs + a settings search box** (maybe group as Project / This device / Global) so you don't tab-hunt.
5. **Un-gate the complex editors** (categories/views/commands/terminals) — editable in place with an inline scope note, no read-only-outside-Resolved.

**Unifying principle:** expose the *effective settings*; make scope a lightweight, inline, mostly-automatic detail — not the primary UI.

**To steer the iteration:** which pain bites most — (a) the Shared/Local/Resolved toggle, (b) too many tabs / hard to find a setting, (c) not knowing where a setting saves, or (d) the complex-editor gating? And are there settings most users should never even see?
