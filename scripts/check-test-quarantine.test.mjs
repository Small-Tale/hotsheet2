import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { listedQuarantines, readSources, sourceMarkers, validateQuarantine } from "./check-test-quarantine.mjs";

const list = (...rows) => `# Quarantine

<!-- quarantine:begin -->
| Ticket | Tier | File | Test | Since | Reason |
|---|---|---|---|---|---|
${rows.join("\n")}
<!-- quarantine:end -->
`;

const rustRow = "| HS2-AAAAAA | rust | `crates/x/src/lib.rs` | `claims_race` | 2026-10-16 | lease timing on a loaded runner |";

test("finds Rust, Vitest, and Playwright markers", () => {
  const markers = sourceMarkers({
    "crates/x/src/lib.rs": '#[test]\n#[ignore = "quarantine HS2-AAAAAA: lease timing"]\nfn claims_race() {}',
    "clients/web/src/a.test.ts": "it.skipIf(quarantined('HS2-BBBBBB'))('flaky', () => {});",
    "clients/web/tests/b.spec.ts": "test('drags a card @quarantine HS2-CCCCCC', async () => {});",
    "clients/web/src/c.test.ts": "it('mentions quarantine in prose only', () => {});",
  });
  assert.deepEqual([...markers].sort(), [
    "HS2-AAAAAA crates/x/src/lib.rs",
    "HS2-BBBBBB clients/web/src/a.test.ts",
    "HS2-CCCCCC clients/web/tests/b.spec.ts",
  ]);
});

test("an empty list with no markers passes", () => {
  assert.deepEqual(validateQuarantine({ "a.rs": "fn main() {}" }, list()), []);
});

test("every marker needs a row and every row a marker", () => {
  const files = { "crates/x/src/lib.rs": '#[ignore = "quarantine HS2-AAAAAA: lease timing"]' };
  assert.deepEqual(validateQuarantine(files, list(rustRow)), []);
  assert.deepEqual(validateQuarantine(files, list()), [
    "quarantine marker without a docs/test-quarantine.md row: HS2-AAAAAA crates/x/src/lib.rs",
  ]);
  assert.deepEqual(validateQuarantine({}, list(rustRow)), [
    "quarantine row without a source marker (remove it once fixed): HS2-AAAAAA crates/x/src/lib.rs",
  ]);
});

test("rows must be complete and the markers present", () => {
  assert.deepEqual(listedQuarantines(list("| HS2-AAAAAA | rust | | x | 2026-10-16 | r |")).failures, [
    "malformed quarantine row: | HS2-AAAAAA | rust | | x | 2026-10-16 | r |",
  ]);
  assert.deepEqual(validateQuarantine({}, "no markers"), ["quarantine list markers are missing"]);
});

test("reads sources under the scan roots and skips build and fixture directories", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hs2-quarantine-"));
  for (const [file, text] of [
    ["crates/x/src/lib.rs", "a"],
    ["crates/x/target/gen.rs", "b"],
    ["clients/web/tests/fixtures/f.ts", "c"],
    ["clients/web/tests/s.spec.ts", "d"],
    ["clients/web/tests/notes.md", "e"],
  ]) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), text);
  }
  assert.deepEqual(readSources(root), { "crates/x/src/lib.rs": "a", "clients/web/tests/s.spec.ts": "d" });
});

test("the repository's quarantine list matches its markers", () => {
  const root = path.resolve(import.meta.dirname, "..");
  assert.deepEqual(validateQuarantine(readSources(root), fs.readFileSync(path.join(root, "docs/test-quarantine.md"), "utf8")), []);
});
