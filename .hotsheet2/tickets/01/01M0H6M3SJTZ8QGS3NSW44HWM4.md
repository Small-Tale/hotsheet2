---
id: 01M0H6M3SJTZ8QGS3NSW44HWM4
slug: HS2-VT3JMF
title: 'Server Tier 1: mTLS + per-device client certs for off-loopback binds'
category: feature
priority: default
status: not_started
created_at: 2026-08-20T04:15:07.827Z
updated_at: 2026-08-20T04:15:07.827Z
legacy_number: HS2-85
schema: 1
---

The v1 server (HS2-7) is Tier 0 only — loopback + shared secret — and REFUSES to bind off-loopback. Build Tier 1 (docs/04 §4.6, docs/08): mTLS + per-device client certs + ACLs (the HS1 per-project CA / .p12 / QR-enrollment / revocation model), so `--bind` off-loopback serves securely instead of refusing. axum + rustls server config; client-cert verification middleware. Follow-up of HS2-7.
