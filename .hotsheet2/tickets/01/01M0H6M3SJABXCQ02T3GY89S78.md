---
id: 01M0H6M3SJABXCQ02T3GY89S78
slug: HS2-BD7Q74
title: 'Multi-viewer PTY sizing: server-arbitrated, focus-follows, leased claims'
category: feature
priority: high
status: not_started
created_at: 2026-08-19T02:46:46.420Z
updated_at: 2026-08-19T03:58:04.901Z
legacy_number: HS2-62
schema: 1
---

Design confirmed-enough (maintainer 2026-08-19), build after terminals (HS2-10). Problem: one PTY = one size, but many viewports (multiple views on one device AND multiple devices incl. remote iOS+macOS) attach at once and need different sizes depending on focus. Fixes HS1's ad-hoc largest/last-writer consensus that never handled remotes. Model: SERVER arbitrates size; each viewport sends a leased size CLAIM over the terminal WS {viewerId(per-viewport, not per-device), cols, rows, focus, visible, activityAt} + heartbeat; server picks size by policy and broadcasts {ptySize, drivenBy}; on disconnect the lease expires and size recomputes (self-heal). Default policy = focus-follows (tmux window-size latest): PTY follows the most-recently-focused viewport; focused/typing viewport's size is locked; hold current size when nothing focused. Guards: SIZE_FOCUS_HOLD_MS ~500ms, SIZE_MIN_DELTA >=2, SIZE_RESIZE_MIN_INTERVAL_MS ~750ms. Alt policies (per-terminal setting): smallest / largest-visible / pinned. Non-driving viewports: letterbox if larger, scale-to-fit-then-scroll if smaller, with a "tap to resize to this screen" affordance. Escape hatch: a natively-sized-per-device need = separate PTYs (HS1 §22.17), not the shared session. This generalizes HS1's borrow-stack (docs/54) to all viewports. See docs/06-clients.md §6.7, docs/05 §5.4.

## Notes

<!-- note: 01M0H6M3SZKF2A6M3MFPC61CS7 -->
2026-08-19T03:58:04.901Z — **Confirmed (maintainer, 2026-08-19):** default sizing policy = **focus-follows**; smallest/largest-visible/pinned are per-terminal alternatives. See docs/06 §6.7.
