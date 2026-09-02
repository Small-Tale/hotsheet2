# 19. Format and rollout compatibility

Status: **Implemented policy and guards; pre-release baseline retained.**

HS2 has two lifecycle phases with deliberately different rules.

## 19.1 Before the first public release

An incompatible ticket, store, project-registry, or settings change is allowed while
the format is still being designed. It must be an explicit migration boundary: announce
the affected formats and required restart/upgrade to the user *before activation*.
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
