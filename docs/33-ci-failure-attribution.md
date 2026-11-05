# CI failure attribution (HS2-HAHX20)

When a batch of integrated tickets shares one CI run and that run fails, `scripts/ci-failure-attribution.mjs` files one follow-up bug per distinct failure and points it back to the most likely source tickets. The attribution is best effort: each suspect carries a stated confidence and the reason for it, and a person or the integrator confirms the cause before acting. Batch integration mode (HS2-9G0M3V) and CI-staged mode (HS2-GT57BQ) call it after a red run; people can run the same command. It does not choose an integration strategy, change coordination state, push, or complete tickets.

## Run it

```sh
# From GitHub: reads every attempt of the run with gh.
node scripts/ci-failure-attribution.mjs --manifest <batch.json> --store <ticket-store> --run <run-id> [--project <checkout>]

# From saved output: the latest attempt's failed log, earlier attempts, and uploaded reports.
node scripts/ci-failure-attribution.mjs --manifest <batch.json> --store <ticket-store> --run-id <run-id> \
  --log <failed.log> [--earlier-log <attempt-1.log>]... [--report <junit.xml|vitest.json|playwright.json>]...

# Print the proposed follow-ups without writing anything.
node scripts/ci-failure-attribution.mjs ... --dry-run
```

`--project` is the checkout at the batch's shipping commit (default: the current directory). `--cli <path>` (or `HOTSHEET_CLI`) chooses the `hotsheet-cli` binary, and `--actor-role`/`--actor-id` are passed through to every ticket write. The command prints one JSON document: the batch id, the run, every proposal with its suspects and reasoning, and, unless `--dry-run`, what was filed. Exit 0 means the attribution ran, whether or not it found failures. Exit 1 means invalid input, a run that has not completed, or a failed `gh`, Git or `hotsheet-cli` call.

The entry point is a root `scripts/*.mjs` command, like `ci-gate.mjs`, `ci-completion-proposal.mjs` and `integration-record.mjs`. It composes `gh`, Git and `hotsheet-cli` the same way, so no new Rust subcommand is needed for headless parity. The pure core (`attribute({ manifest, parsed, run, repo })`) and the filer (`fileAll`) are exported for batch mode to call in-process.

## Batch manifest

Batch mode writes the manifest. Version 1:

```json
{
  "version": 1,
  "batch_id": "batch-0001",
  "project_id": "<checkout-id>",
  "shipping_sha": "<batch main SHA, optional>",
  "tickets": [
    {
      "slug": "HS2-ABC123",
      "ticket": { "project_id": "<checkout-id>", "connection_id": "<connection>", "native_id": "<native id>" },
      "source_commits": ["<full sha>"],
      "shipping_commits": ["<full sha>"],
      "changed_files": ["optional/list/of/paths"]
    }
  ]
}
```

Tickets are routed by their qualified `(connection_id, native_id)`; the slug is only for display. Without `changed_files`, the files come from `git diff-tree` of each shipping commit in `--project`. The manifest order is the batch order, and it breaks ranking ties.

## Failures

`scripts/ci-failure-parse.mjs` reads the output of `gh run view <run> --attempt <n> --log-failed` (lines of job, step, timestamp and text) and test reports:

| Source                                           | Failure identity                               |
| ------------------------------------------------ | ---------------------------------------------- |
| cargo-nextest (`FAIL`, `TIMEOUT`, signals)       | binary id and test path                        |
| vitest (`FAIL path > test`, JSON report)         | test file and test name                        |
| Playwright (list output, JSON report)            | spec file and title, merged across browsers    |
| `node --test` (spec summary, TAP)                | test file and test name                        |
| rustc/clippy, `cargo fmt`, tsc, ESLint, Prettier | job and step, with the reported file locations |
| JUnit XML (nextest's report)                     | binary id and test name                        |
| any other failed step                            | job and step                                   |

A test is the same failure whichever job or shard ran it. Only identifiers, file paths and one short excerpt (the identifier line itself, at most 160 characters) leave the parser; no other log text reaches a ticket.

## Ranking

For each failure, every batch ticket gets its strongest signal:

| Signal                 | Weight | Meaning                                                                                                                        |
| ---------------------- | -----: | ------------------------------------------------------------------------------------------------------------------------------ |
| `failure_file_changed` |    0.9 | The ticket changed the failing test file or a file a diagnostic names.                                                         |
| `imports_changed`      |    0.6 | The failing script test's relative runtime imports reach a changed file.                                                       |
| `exercised_by_spec`    |    0.6 | The HS2-N7EY18 browser exercise index (`scripts/gate-browser-exercise.mjs`) selects the failing spec for the ticket's changes. |
| `same_crate`           |   0.45 | The ticket changed the failing Rust test's crate.                                                                              |
| `crate_dependency`     |    0.3 | The ticket changed a workspace crate the failing crate depends on.                                                             |
| `area_owner`           |    0.2 | The ticket touched the failure's area: its crate, package or top-level directory, or the area implied by the job name.         |
| `no_signal`            |    0.1 | Nothing connects them.                                                                                                         |

The repository has no CODEOWNERS file, so path ownership means the area that owns a path. Coverage data is not read yet (HS2-73RY95). When any ticket has a signal, only the tickets with a signal are suspects. Otherwise every batch ticket is an equal suspect. A suspect's confidence is its weight divided by the larger of 1 and the suspects' total weight, labelled high (60% or more), medium (30% or more) or low. Thus one strong suspect stays high, tied suspects split the share, and a lone weak signal stays low. A path that matches no repository file is listed as unknown, and the failure falls back to the job's area.

## Follow-ups

Each distinct failure gets one high-priority bug tagged `ci-failure-attribution`. It lists the failure identity, job and step, resolved and unknown files, batch id, CI run (link, attempt and head), the excerpt, a table of ranked suspects with qualified ids, confidence and reasons, and a one-line reasoning. Each suspect in the same store receives a typed follow-up link (`hotsheet-cli link-follow-up`) and a `## CI failure suspect` back-link note.

Writes are idempotent by run and failure identity. The bug's body ends with a `ci-failure-attribution: <key>` marker, where the key is a hash of the run id and failure identity. A re-run finds the bug by tag and marker, then adds only missing links and notes. An interrupted run is repaired by running it again. A suspect on another connection is not guessed at: it is listed in the bug and reported under `unsupported` in the output, because a cross-connection link needs that provider's own write path.

## Flaky failures

A failure that appears in an earlier attempt but not in the latest, a run whose latest attempt is green, a nextest retry that passed, and a Playwright `flaky` entry are recorded as flaky. They get no attributed follow-up. As [test-quarantine.md](test-quarantine.md) requires for a test that needed a retry, each one gets a flake bug tagged `ci-failure-attribution` and `flaky-test`, with no suspect links. Its key is the failure identity alone, so a later run reuses the ticket and adds one occurrence note per run.

## Not yet built

- Optional bisect of suspect commits on a scratch branch, in the background, updating the follow-up with the result: HS2-9PHHHS.
- Coverage-data and import-graph signals beyond relative script imports, the browser exercise index and the Rust crate graph: HS2-73RY95.

Batch mode supplies the manifest and invokes the command after a red covering run (HS2-9G0M3V). Its completion rule, that a ticket completes only once each failure its batch caused has a linked follow-up, reads the `filed` output.
