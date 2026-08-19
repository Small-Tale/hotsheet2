# 10. Assignment & Human-in-the-Loop Collaboration

> **Status: Open (design proposal).** Addresses the maintainer's note (2026-08-19):
> *"need to think about ticket assignment/claiming in a distributed setting and
> tickets that humans need to be assigned to / in the loop for. On teams we might
> want the specific attention of one or more people to either directly do work or
> to provide feedback. Need to figure out how that works with git-based tickets."*
> Tracked: HS2-20.

## 10.1 Two different "who's on this?" concepts — keep them separate

HS1 has exactly one mechanism (`claim/lease`) and it is about **machine workers**
draining a pool. Teams need a **second, orthogonal** mechanism about **people**.
Conflating them is the trap.

| | **Claim / lease** (machines) | **Assignment** (humans) |
|---|---|---|
| Who | An AI worker / agent process | A named person |
| Purpose | Prevent two workers doing the same ticket right now | Direct a person's attention to do work or give feedback |
| Lifetime | Seconds–minutes, **expiring** (auto-reclaimed) | Durable until the person acts / it's cleared |
| Storage | `claim_*` frontmatter, ephemeral | `assignees` / `review_requests` frontmatter, shared |
| Distributed role | Coordination correctness | Team workflow / notification |

A ticket can have both at once: assigned to *Dana* (human) while an *agent* holds a
live claim doing the mechanical part. They don't contend.

## 10.2 The data model (shared, in the ticket file)

Two frontmatter fields, both **Tier A shared** ([02](02-ticket-storage.md) §2.11)
so every teammate sees them and they sync + merge automatically:

```yaml
assignees: [dana@example.com, brian@example.com]   # people expected to do the work
review_requests:                                   # people whose input is wanted
  - { who: dana@example.com, kind: feedback, by: 01J9ZK…req-ulid, at: 2026-08-19T… }
  - { who: sam@example.com,  kind: review,   by: 01J9ZK…req-ulid, at: 2026-08-19T… }
```

- **`assignees`** — a set of person identities expected to *do* the ticket.
- **`review_requests`** — a set of "I need this person in the loop" asks, each with
  a **kind**: `feedback` (weigh in), `review` (approve/verify), or `fyi` (awareness).
  Each request carries its own ULID `by`, so requests **merge by set-union** exactly
  like notes — two people adding a reviewer never conflict.
- **Person identity = the git identity (email)** by default, optionally mapped to a
  friendly name via a **project roster** (`roster` in project config, or a
  `people.json` in the store) — so `dana@example.com` renders as "Dana". Using the
  git identity means assignment is meaningful across clones and needs no separate
  accounts system (a chartered non-goal — [00](00-vision-and-principles.md) §0.3).

Assignment maps naturally onto HS1's existing `FEEDBACK NEEDED:` note convention
(the worklist already uses it) — `review_requests[kind=feedback]` is the
structured, assignable version of that.

## 10.3 How a person is "gotten" (attention / notification)

Because tickets are just git, "notifying" is layered:

1. **In-app surfacing (always):** derived views off the index —
   **"Assigned to me"**, **"Needs my feedback / review"**, **"I requested"** —
   scoped to the current user's identity. This works with zero extra
   infrastructure; the index already has the fields.
2. **Live push (when connected):** a client attached to a server that owns the
   project gets a WebSocket event when it's newly assigned/requested — the same bus
   that carries index changes ([04](04-core-server-cli.md) §4.3).
3. **Cross-device / offline (later):** for a teammate not currently connected, the
   attention rides the **sync engine** ([02](02-ticket-storage.md) §2.12) — the
   assignment is committed + pushed, and their Hot Sheet picks it up on next
   fetch and raises a local/desktop (or, later, iOS push) notification. No central
   server required; the git remote is the delivery channel.

So the *record* of "you're wanted" is always in git (durable, offline-safe); the
*alerting* is best-effort on top (live when connected, on-sync otherwise).

## 10.4 Distributed claiming (machines) — restated for this setting

The claim/lease primitive ([05](05-ai-tool-plugins.md) §5.7) already handles the
machine side, and on git storage the **claim state is persisted to ticket
frontmatter** so it's visible across the team while remaining expiring/ephemeral.
Two refinements for the distributed/team case:

- **Human assignment can scope machine claiming.** A `claim-next` can be filtered to
  "tickets assigned to me / my team / unassigned," so a person's agents drain that
  person's queue — assignment becomes an input to worker selection, not a competitor
  to it.
- **A human assignment is not a lease.** It does not block a machine worker or
  expire; it's a durable "this person owns the outcome." The write-conflict guard
  (which protects against two *simultaneous editors*) still applies regardless of
  who's assigned.

## 10.5 Open questions (for HS2-20)

- **Identity mapping.** Git email as the key + a roster for display — confirm, and
  decide where the roster lives (project config vs. a committed `people.json` in the
  store so it syncs).
- **"Assign to" vs. "request review" UX.** One control with a kind, or two? Lean:
  one "People…" control that sets assignees + adds review requests with a kind.
- **Notification transport off-server.** In-app + on-sync desktop notifications are
  clear; the iOS-push path depends on the deferred push infrastructure
  ([08](08-distributed-and-remote.md) O5).
- **Team roster source of truth.** For a GitHub-backed store, could we derive people
  from repo collaborators? Optional enhancement; git identity works without it.
- **Interaction with `blocked_by`.** A `review` request that must complete before a
  ticket proceeds — is that a `blocked_by` edge, or a softer gate? Lean: keep review
  requests soft (attention), use `blocked_by` for hard ordering.

## 10.6 Cross-references
- The `assignees` / `review_requests` fields + shared/local tiering: [02](02-ticket-storage.md) §2.5, §2.11
- **Close reasons** — the other collaboration-motivated field (why a shared ticket
  was closed: completed / not_planned / duplicate): [02](02-ticket-storage.md) §2.6a
- Machine claim/lease: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7
- Attention delivery via sync + push: [02](02-ticket-storage.md) §2.12, [08-distributed-and-remote.md](08-distributed-and-remote.md)
