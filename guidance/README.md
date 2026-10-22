# Built-in guidance provenance

For opt-in Rulesync consumption and writer ownership, see the
[validated consumer guide](rulesync/README.md). The built-in path below is the default.

The maintained reusable prose lives in
[`Small-Tale/ai-playbook`](https://github.com/Small-Tale/ai-playbook).
`ai-playbook.lock.json` pins the source commit, release label, rule names, versions,
and SHA-256 hashes. The files in `plugins/shared/` and the four first-party
`plugins/*/instructions.md` files are distributable snapshots. Cargo embeds those
existing paths, so installed setup and refresh stay headless and offline. They do
not invoke Rulesync, fetch source, or need a local playbook checkout.

The v1.2.0 source catalog offers 18 selectable rules. The built-in offline bundle
still contains its five established Hot Sheet rules; the other rules are selected
per project from a registered pinned source. The snapshot updater reads those five
entries from the larger manifest and keeps the previous exact bodies as migration
fixtures, so existing projects can refresh without losing local guidance.

Check the installed snapshots offline:

```sh
node scripts/sync-ai-playbook.mjs --check
node --test scripts/sync-ai-playbook.test.mjs
```

CI additionally runs `--check --fetch`, which reads the exact pinned public upstream
commit and verifies the lock and every bundled body against it. No moving branch or
release lookup participates in that check. A source checkout can replace the fetch:

```sh
node scripts/sync-ai-playbook.mjs --check --source ../ai-playbook
```

To update, first change and release the canonical rules in ai-playbook. Review its
section-version changes and then regenerate from the full source commit:

```sh
node scripts/sync-ai-playbook.mjs --write --source ../ai-playbook \
  --ref <full-40-character-commit> --release <immutable-release-tag>
```

The script reads committed files at that SHA, so uncommitted source edits cannot
leak into a snapshot. Review the lock and all destination diffs. When a section body
changes, retain its previous exact body as a `plugins/shared/legacy-*.md` migration
fixture and register it in `crates/hotsheet-plugins/src/setup.rs`. Bump the upstream
section version; workflow changes also bump the common instruction version. Update
product docs and setup/refresh tests in the same ticket. Never change snapshot prose
independently in Hot Sheet 2.

The initial v1.0.0 snapshot is byte-identical to the preceding HS2 bundle. Existing
managed markers, independent section selection, nested `hotsheet:specifics`, local
customizations, equal/newer-version protection, and disabled-tool cleanup therefore
keep their existing migration behavior. Legacy fixtures remain in HS2 because the
installed writer owns backward compatibility. Plugin declarations, provider-specific
adapters, skill templates, and HS2 engineering policy also remain here.
