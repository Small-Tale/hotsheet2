# Unresolved claims

Claims an author could not verify at the audited source revision. They are
not asserted in the chapters. Append under a heading for your ticket; never
rewrite another ticket's entries. Close an entry by striking it through and
naming where it was resolved.

| Id    | Area       | Claim                                                                                                                                                                                                                  | Why it is unverified | Follow-up |
| ----- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- | --------- |
| WC-U1 | web-client | Whether any shipped configuration enables the change-stream worker outside profiling (`npm run profile:ui:worker`). The code default is off; user settings cannot be observed from the repository.                     | Stated in the claim  | none      |
| WC-U2 | web-client | The signal count (about 233 in the runtime) is a grep count of `signal(` calls, not a semantic inventory.                                                                                                              | Stated in the claim  | none      |
| WC-U3 | web-client | Upstream status of the KF-* tickets named in the web-client chapter; Kerf's tracker is not in this repository.                                                                                                         | Stated in the claim  | none      |
| WC-U4 | web-client | Terminals hidden by visibility groups are never mounted, so they are never attached; this is inferred from the filter in `clients/web/src/terminal-visibility.ts:6` and the absence of a detach path, not from a test. | Stated in the claim  | none      |
