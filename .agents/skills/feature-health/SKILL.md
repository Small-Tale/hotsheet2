---
name: feature-health
description: Re-assess every feature for whether it actually works (verified, unverified, broken) from automated coverage, fix-commit history, the open backlog, manual-only surfaces, and real use, then rewrite the project's feature-health document wholesale and report the deltas since the last snapshot.
allowed-tools: Read, Grep, Glob, Bash, Edit, Write
---

<!-- ai-playbook:skill name=feature-health v=1 -->

Rebuild the project's feature-health document (`docs/feature-health.md` unless the docs index
names another location) from current evidence.

## The question this answers

**Does each feature actually work?** Not "was it built?" The requirements summary that the
`hotsheet-requirements-documentation` rule requires already tracks built-ness (shipped,
partial, design-only, deferred). This document tracks **trustworthiness**. A feature can be
fully shipped there and `Unknown` here: built, never realistically exercised. That gap is the
entire point.

**Enhancements are out of scope.** Every feature could do more; saying so is noise. A note
earns its place only if it records:

- missing **core** functionality (the feature cannot do its stated job);
- a **known bug** or a burst of fix commits;
- an **unsettled decision** the feature is resting on;
- **absent verification** (no automated coverage and no evidence of real use).

A feature with nothing wrong gets **no note**. Resist the urge to fill every cell.

## Method

Work from evidence, in this order of weight. Do not skip to conclusions from the docs; they
have been wrong in both directions before.

1. **Enumerate the features.** Use the requirements summary as the spine, then merge documents
   into coherent user-facing features. Also sweep for features with no document (client
   modules, settings tabs, CLI subcommands). Keep the existing table's grouping unless the
   product has genuinely changed shape.
2. **Map automated coverage.** List the end-to-end specs and unit test files. For each feature
   ask: is there a spec that drives the **real user flow**?
   - **Open the spec and read its test names** before crediting it. A spec that only asserts
     empty states, disabled controls, or button visibility does not cover the feature.
   - Specs that need credentials or are excluded from the fast suite are weaker evidence.
   - The feature coverage record, when the project keeps one, is a lead to verify, not proof.
3. **Find where the bugs currently are.** `git log --oneline -200 --format='%s'`. A cluster
   of fix commits on a feature that shipped in the last couple of weeks means **Shaking out**.
   Check dates with `git log -1 --format=%as -- <path>` when unsure.
4. **Read the open backlog.** Query Hot Sheet for open and backlog tickets. Look specifically
   for "decide whether to" tickets (the feature rests on an **unsettled decision**, so it is
   Underbaked regardless of code completeness), known-broken reports, and deferred
   sub-tickets that leave a footgun.
5. **Identify manual-only surfaces.** Anything automation cannot verify: native shells,
   keychain backends, real AI agents, exposed network modes, the OS clipboard, notifications.
   The manual test plan is the catalog; note that its checkboxes are never ticked, so it
   proves what cannot be automated, never what was verified.
6. **Diff advertisements against implementations.** Docs and type unions routinely promise
   more than the code delivers: a UI element union versus what the renderer handles, a
   dropdown of options versus the ones with a real implementation. `ast-grep` is the right
   tool for these structural diffs. Re-check every instance the document already lists and
   look for new ones.
7. **Weigh dogfooding.** Anything exercised constantly by the team's own daily use has strong
   real-world evidence even where tests are thin. A demo mode, a test mode, or a telemetry
   dashboard does not get this credit.

## Assigning a status

Use the vocabulary already in the document and keep its definitions table intact: **Solid**,
**Solid\*** (solid with a caveat), **Shaking out**, **Underbaked**, **Incomplete**, **Unknown**,
**Not built**. If the document does not exist yet, create it with that table.

Two calibration rules matter more than the definitions:

- **Absence of evidence is `Unknown`, not `Solid`.** If nothing shows a feature has ever been
  run for real, say so. Do not assume it works because it compiles and has unit tests.
- **`Underbaked` is about trust, not completeness.** A fully built feature with an open
  "should we keep this?" ticket, a wrong default, or an unsettled model underneath it is
  Underbaked.

## Writing the document

1. **Rewrite it wholesale.** This is a snapshot, not an accreting log. Set the snapshot date at
   the top to today.
2. Recount the **dashboard** table from the rows you actually wrote; do not carry old numbers
   forward.
3. Refresh the **shortlist**: the five to eight things worth fixing or verifying first,
   ordered by how much risk they carry. Structural problems that will recur outrank one-off
   gaps.
4. Keep the **"How this assessment is made"** section current: a new evidence source or a new
   advertise-versus-implement trap goes there so the next run inherits it.
5. Cite specifics: ticket slugs, file paths, spec names, commit counts. A note a reader cannot
   verify is worth little.

## Afterwards

- **Report the deltas** since the last snapshot: what improved, what regressed, what is newly
  Shaking out. That change-over-time signal is the main reason to run this repeatedly.
- **File tickets for concrete defects** you confirm along the way and reference them in the
  notes.
- Do **not** file tickets for things that are merely `Unknown`; the fix for those is
  verification, so prefer one ticket per *area* to actually exercise it rather than noise.
- A docs-only run needs no lint or type gate; run them only if you changed code.
