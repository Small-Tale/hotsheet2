# HS2-63 — Git-native claim/lease spike

Validates the **git-native distributed claim/lease** mechanism from
[`docs/08-distributed-and-remote.md`](../../docs/08-distributed-and-remote.md) §8.5:
a per-ticket claim marker claimed by an **atomic-push compare-and-swap**, so multiple
machines coordinate over a shared git remote with **no central coordinator**.

## Result (2026-08-19): validated ✅

**Decision:** use a **custom ref namespace `refs/hotsheet/claims/<ulid>`** as the
primary marker; **reserved-prefix tags** are the documented fallback for any remote
that rejects custom refs. Semantics are identical either way (a git server-side
guarantee).

### Mechanism proof (`git-claim-spike.sh` — generic bare repo, no network)
- Custom-ref CAS: 1st claimant wins, 2nd **rejected** ("fetch first") ✅
- Tag CAS: 2nd **rejected** ("already exists") ✅
- `--force-with-lease` renew by the owner ✅
- **Two-stealer race on an expired claim → exactly one wins** (the stale stealer is
  rejected "stale info") ✅
- `git ls-remote 'refs/hotsheet/claims/*'` enumerate + delete/sweep ✅

### GitHub support (`github-probe.sh` — live, touches the remote)
GitHub **accepts custom ref namespaces** — `refs/hotsheet/claims/*` pushed, listed,
and deleted cleanly (contradicts the older "GitHub only allows heads/tags" lore).
Tags also work. This is why the custom ref is preferred: it's invisible to
`git tag`/`branch` and the GitHub UI, and bulk-prunable.

## How to run

```sh
# Mechanism proof — safe, self-contained, no network:
bash git-claim-spike.sh /tmp/hs-claim-spike-run

# GitHub probe — ⚠ pushes TWO throwaway refs to `origin` then deletes them.
# Run only against a repo you own / with a remote you can push to.
bash github-probe.sh
```

## Caveats
- Claiming is **online** (a push round-trip). Custom refs aren't fetched by the
  default refspec, so the protocol explicitly `ls-remote`s / fetches the
  `refs/hotsheet/claims/*` namespace.
- A concurrent push race **against GitHub specifically** wasn't stress-tested (it's a
  git protocol guarantee); revisit only if real contention shows anomalies.
