---
id: 01M0H6M3SJBEBCCD2CMJPSKA5T
slug: HS2-5JJA96
title: 'Server: multi-store (one server per machine serves all local projects) + instance registry'
category: feature
priority: default
status: not_started
created_at: 2026-08-20T04:15:46.851Z
updated_at: 2026-08-20T04:15:46.851Z
legacy_number: HS2-87
schema: 1
---

The v1 server (HS2-7) serves ONE store (its `-C` path). The topology decision (HS2-7 note, docs/04 §4.3) is ONE server per machine serving ALL local projects, with clients/CLI joining it. Build: register multiple stores, route requests per project (path/subdomain/header selecting the store), and a machine-global instance file (`~/.hotsheet/instance.json` holding the port) for discover-and-join. Overlaps the lifecycle work in HS2-59. Follow-up of HS2-7 / HS2-59.
