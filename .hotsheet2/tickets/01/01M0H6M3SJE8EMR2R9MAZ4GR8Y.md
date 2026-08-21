---
id: 01M0H6M3SJE8EMR2R9MAZ4GR8Y
slug: HS2-S009CK
title: 'Distributed claim/lease: git-ref CAS on top of the local claim primitive'
category: feature
priority: default
status: not_started
created_at: 2026-08-20T02:56:03.465Z
updated_at: 2026-08-20T02:56:03.465Z
legacy_number: HS2-84
schema: 1
---

HS2-83 built the LOCAL claim/lease primitive in the CLI (`claim-next`/`release`/`renew` write the claim frontmatter fields; picks open + unblocked + unclaimed-or-expired). That's correct for a single worker on a local store but gives no cross-worker safety. Layer the distributed design on top: atomic-push CAS on a per-ticket claim ref (`refs/hotsheet/claims/<ulid>`, validated in the HS2-63 spike — GitHub accepts custom refs; tags fallback), lease renewal/expiry semantics, poison-retry via claim_count, and reconciliation of the ref state with the ticket file. Wire it through the server (HS2-7) and the CLI's claim commands. See docs/05 §5.7, docs/08, and spikes/hs2-63-git-native-claim/. Follow-up of HS2-83.
