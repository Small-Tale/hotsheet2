---
id: 01M0H6M3SJ1D7NHZH7VQK9FAH8
slug: HS2-ED8RY0
title: Terminal/PTY manager + detached broker + byte-stream busy inference
category: feature
priority: default
status: not_started
created_at: 2026-08-19T00:23:25.256Z
updated_at: 2026-08-19T02:46:48.509Z
legacy_number: HS2-10
schema: 1
---

Port HS1's terminal system onto the new core: per-project PTYs (lazy spawn, scrollback ring, multi-viewer attach), a detached broker process so terminals survive server restarts, env scrubbing, OSC 7/8/9/133 handling, and byte-stream spinner busy inference feeding the connection registry. PTY SIZING is server-arbitrated via leased focus-follows claims — its own ticket HS2-62 (replaces HS1's largest/last-writer consensus; works across remote+local viewports). See docs/05-ai-tool-plugins.md §5.4, §5.6, docs/06 §6.7.
