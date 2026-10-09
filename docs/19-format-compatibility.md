# 19. Format and rollout compatibility

Status: **Implemented policy and guards; pre-release baseline retained.**

HS2 has two lifecycle phases with deliberately different rules.

## 19.1 Before the first public release

An incompatible ticket, store, project-registry, or settings change is allowed while
the format is still being designed. It must be an explicit migration boundary: announce
the affected formats and required restart/upgrade to the user _before activation_.
Ordinary reads and writes must not silently activate a breaking marker underneath a
running process. A development source edit is not activation; rebuilding/restarting or
running an explicit migration is. The retained `prerelease-*` fixtures are a regression
baseline, not a promise that every pre-release byte shape is permanent.

## 19.2 Starting with the first public release

Every release permanently reads every ticket, `hotsheet-store.json`, checkout/project
registry, and global/shared/local settings format emitted by every older HS2 release.
Before publishing a release, add an immutable, release-named fixture corpus containing
all four persisted surfaces. CI must run the current readers against every retained
corpus. Never edit an old fixture to make a test pass; add migration/read compatibility.

Writers may add optional fields that old readers preserve or ignore. A genuinely
incompatible new shape requires a new explicit version marker. Readers encountering a
marker above their supported range return the stable `upgrade_required` classification
and the message that the item was created by a newer Hot Sheet 2, cannot be opened by
this version, and requires an update. It is never labeled generic corruption.

## 19.3 Unsynchronized rollout

Client and server deployment is always assumed unsynchronized, including app-store
rollouts. The authenticated `/compatibility` handshake advertises inclusive protocol
and store-reader ranges. Intersecting protocol ranges proceed regardless of exact build
revision. Non-intersecting ranges stop before project API use with client-update or
server-update guidance. Missing metadata remains an explicit compatibility-unknown
legacy state; it must not cause a retry loop. No behavior may depend on simultaneous
client/server installation.

Persisted-format guards are independent of the API protocol. Current readers accept
unversioned legacy settings/project registries and write version 1 markers. They reject
future markers with upgrade guidance. Ticket diagnostics expose `error_code` as either
`invalid_ticket` or `upgrade_required`, allowing every client to preserve healthy rows
while presenting newer tickets accurately.

Git store metadata now includes an optional durable `instanceId`; existing schema-3
stores remain readable, and a linked legacy store receives the field while its old
path is available. Checkout registry schema 3 adds optional
`store_instance_ids` entries keyed by Git connection id. Relink requires those
identities to match, so a legacy link whose store moved before migration needs
an explicit recovery path (HS2-EFBAPC, HS2-RAQSX7). Older writers should be
stopped before a move so they cannot rewrite these optional identity fields.

Store-schema compatibility is checked before a store is attached to a server, not on its
first ticket mutation. `hotsheet-cli compatibility --json` exposes the schema the current
headless bootstrap creates and, when `-C` selects a store, that store's schema. Graphical
bootstrap compares it with the active project's authenticated server range before writing
the new repository. If an older detached server cannot host it, creation is refused with
project-scoped restart guidance and no partially usable store is left behind.

Attachment annotations with explicit shapes use the guarded ticket marker
`hotsheet/v3-annotation-shapes`. The marker is introduced when a ticket first contains
a shape field and remains when shapes are later removed. A v2 reader rejects that
string before deserializing or rewriting the
ticket, so it cannot discard the geometry. Rectangle-only files continue to use
`hotsheet/v2-bounded-notes` without a store-wide activation.

Tickets with explicit media annotation intents use the sticky
`hotsheet/v4-annotation-intents` marker. An older v3 shape-aware binary rejects
the ticket before it can discard the intent list. Empty intents remain the
shape default and do not trigger this guard on a previously v2/v3 ticket.
