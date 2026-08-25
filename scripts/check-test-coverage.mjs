import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const allowed = new Set(["double-covered", "unit-only", "e2e-only", "manual", "planned"]);

export function validateMatrix(root, text) {
  const body = text.match(/<!-- coverage-matrix:begin -->([\s\S]*?)<!-- coverage-matrix:end -->/)?.[1];
  if (!body) return { count: 0, failures: ["coverage matrix markers are missing"] };
  const rows = body.split("\n").filter((line) => /^\| [a-z0-9-]+ \|/.test(line));
  if (!rows.length) return { count: 0, failures: ["coverage matrix has no feature rows"] };
  const ids = new Set();
  const failures = [];

  function evidence(cell, id, layer) {
    if (cell === "—") return false;
    for (const raw of cell.split(";")) {
      const ref = raw.trim().replaceAll("`", "").split("#", 1)[0].trim();
      if (!ref || !fs.existsSync(path.join(root, ref))) failures.push(`${id}: missing ${layer} evidence ${ref}`);
    }
    return true;
  }

  for (const row of rows) {
    const cells = row.split("|").slice(1, -1).map((cell) => cell.trim());
    if (cells.length !== 7) { failures.push(`malformed row: ${row}`); continue; }
    const [id, requirement, , unitCell, e2eCell, manualCell, status] = cells;
    if (ids.has(id)) failures.push(`${id}: duplicate id`); else ids.add(id);
    if (!fs.existsSync(path.join(root, requirement))) failures.push(`${id}: missing requirement ${requirement}`);
    if (!allowed.has(status)) failures.push(`${id}: invalid status ${status}`);
    const unit = evidence(unitCell, id, "unit");
    const e2e = evidence(e2eCell, id, "E2E");
    evidence(manualCell, id, "manual");
    if (status === "double-covered" && !(unit && e2e)) failures.push(`${id}: double-covered requires unit and E2E evidence`);
    if (status === "unit-only" && !(unit && !e2e)) failures.push(`${id}: unit-only status/evidence disagree`);
    if (status === "e2e-only" && !(!unit && e2e)) failures.push(`${id}: e2e-only status/evidence disagree`);
  }
  return { count: rows.length, failures };
}

if (fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const root = path.resolve(import.meta.dirname, "..");
  const result = validateMatrix(root, fs.readFileSync(path.join(root, "docs/TEST-COVERAGE.md"), "utf8"));
  if (result.failures.length) {
    console.error(result.failures.join("\n"));
    process.exit(1);
  }
  console.log(`Validated ${result.count} feature coverage rows.`);
}
