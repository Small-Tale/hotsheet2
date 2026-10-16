// Flaky-test quarantine check (HS2-3PX1E4). A known flake is skipped by default through a marker
// that names its fix ticket, and every marker has a row in docs/test-quarantine.md (and every
// row a marker), so quarantined tests stay visible and tracked instead of being retried silently.
//
// Markers:
//   Rust        #[ignore = "quarantine HS2-XXXXXX: reason"]
//   Vitest      it.skipIf(quarantined('HS2-XXXXXX'))('name', ...)   (clients/web/src/test-quarantine.ts)
//   Playwright  test('name @quarantine HS2-XXXXXX', ...)            (playwright.config.ts grepInvert)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const SCAN_ROOTS = ["crates", "clients/web/src", "clients/web/tests", "clients/web/scripts", "migrator/src", "migrator/test"];
const SOURCE = /\.(rs|ts|tsx|mjs|js)$/;
const SKIP = new Set(["node_modules", "target", "dist", "fixtures"]);
/** The helper's own tests use sample slugs, not real quarantines. */
const SKIP_FILES = new Set(["clients/web/src/test-quarantine.test.ts"]);
const MARKER = /(?:"quarantine |@quarantine |quarantined\(\s*['"])(HS2-[0-9A-Z]+)\b/g;

/** Every quarantine marker in `files` ({path: source}) as `slug path` keys. */
export function sourceMarkers(files) {
  const markers = new Set();
  for (const [file, source] of Object.entries(files)) {
    for (const match of source.matchAll(MARKER)) markers.add(`${match[1]} ${file}`);
  }
  return markers;
}

/** Rows of the tracked list between its markers: `| Ticket | Tier | File | Test | Since | Reason |`. */
export function listedQuarantines(text) {
  const match = text.match(/<!-- quarantine:begin -->([\s\S]*?)<!-- quarantine:end -->/);
  if (!match) return { rows: null, failures: ["quarantine list markers are missing"] };
  const rows = new Set();
  const failures = [];
  for (const line of match[1].split("\n")) {
    if (!/^\|\s*HS2-/.test(line)) continue;
    const cells = line.split("|").slice(1, -1).map((cell) => cell.trim().replaceAll("`", ""));
    if (cells.length !== 6 || cells.some((cell) => cell === "")) {
      failures.push(`malformed quarantine row: ${line}`);
      continue;
    }
    rows.add(`${cells[0]} ${cells[2]}`);
  }
  return { rows, failures };
}

export function validateQuarantine(files, listText) {
  const { rows, failures } = listedQuarantines(listText);
  if (!rows) return failures;
  const markers = sourceMarkers(files);
  for (const key of markers) if (!rows.has(key)) failures.push(`quarantine marker without a docs/test-quarantine.md row: ${key}`);
  for (const key of rows) if (!markers.has(key)) failures.push(`quarantine row without a source marker (remove it once fixed): ${key}`);
  return failures;
}

export function readSources(root, roots = SCAN_ROOTS) {
  const files = {};
  const walk = (dir) => {
    let entries;
    try {
      entries = fs.readdirSync(path.join(root, dir), { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const rel = `${dir}/${entry.name}`;
      if (entry.isDirectory()) {
        if (!SKIP.has(entry.name)) walk(rel);
      } else if (SOURCE.test(entry.name) && !SKIP_FILES.has(rel)) {
        files[rel] = fs.readFileSync(path.join(root, rel), "utf8");
      }
    }
  };
  for (const dir of roots) walk(dir);
  return files;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const failures = validateQuarantine(readSources(root), fs.readFileSync(path.join(root, "docs/test-quarantine.md"), "utf8"));
  if (failures.length) {
    console.error(failures.join("\n"));
    process.exit(1);
  }
  console.log("test quarantine list matches the source markers");
}
