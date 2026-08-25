import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { validateMatrix } from "./check-test-coverage.mjs";

function fixture(status = "double-covered", e2e = "`e2e.rs`") {
  return `<!-- coverage-matrix:begin -->
| ID | Requirement | Feature | Unit evidence | E2E evidence | Manual evidence | Status |
|---|---|---|---|---|---|---|
| feature-a | req.md | A feature | \`unit.rs\` | ${e2e} | — | ${status} |
<!-- coverage-matrix:end -->`;
}

test("accepts a coherent double-covered feature", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hs2-coverage-"));
  for (const file of ["req.md", "unit.rs", "e2e.rs"]) fs.writeFileSync(path.join(root, file), "");
  assert.deepEqual(validateMatrix(root, fixture()), { count: 1, failures: [] });
});

test("rejects a dishonest double-covered status", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hs2-coverage-"));
  for (const file of ["req.md", "unit.rs"]) fs.writeFileSync(path.join(root, file), "");
  const result = validateMatrix(root, fixture("double-covered", "—"));
  assert.ok(result.failures.some((failure) => failure.includes("requires unit and E2E")));
});
