import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

// kerf-ui-doctor exits non-zero only for errors; review findings and warnings always exit 0. Until
// Kerf ships a failure threshold (KF-6S5EKX), this gate fails on any active diagnostic. Known,
// tracked gaps are declared as documented `suppressions` in .kerf-ui-doctor.json, never here.
export function assertKerfUiDoctorClean(report) {
  if (report.schemaVersion !== 1) throw new Error(`Unsupported Kerf UI doctor report schema ${report.schemaVersion}.`);
  if (report.exitCode !== 0 && report.exitCode !== 1)
    throw new Error(`Kerf UI doctor did not complete successfully (exit ${report.exitCode}).`);
  const active = report.diagnostics.filter((diagnostic) =>
    ['error', 'review', 'warning'].includes(diagnostic.severity),
  );
  if (active.length)
    throw new Error(
      `Kerf UI doctor found ${active.length} active diagnostic(s):\n${active
        .map((diagnostic) => `${diagnostic.severity} ${diagnostic.id}: ${diagnostic.message}`)
        .join('\n')}`,
    );
  return report.summary;
}

function run() {
  const workspace = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const temporary = mkdtempSync(join(tmpdir(), 'hotsheet-kerf-doctor-'));
  const output = join(temporary, 'report.json');
  try {
    const cli = resolve(workspace, 'node_modules/@kerfjs/ui/doctor/cli.mjs');
    const result = spawnSync(process.execPath, [cli, '--full', '--format', 'json', '--output', output], {
      cwd: workspace,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    if (result.error) throw result.error;
    const summary = assertKerfUiDoctorClean(JSON.parse(readFileSync(output, 'utf8')));
    console.log(
      `Kerf UI doctor clean: ${summary.errors} errors, ${summary.review} review findings, ${summary.warnings} warnings, ${summary.suppressed} documented suppressions.`,
    );
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) run();
