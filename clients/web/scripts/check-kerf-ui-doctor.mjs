import { readFileSync, rmSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const KERF_UI_DOCTOR_BUDGET = {
  error: {
    'KUI-L017': 1,
    'KUI-L101': 3,
    'KUI-L102': 8,
    'KUI-L201': 22,
    'KUI-L202': 4,
    'KUI-L203': 1,
  },
  review: {
    'KUI-L004': 81,
    'KUI-L005': 54,
    'KUI-L006': 15,
    'KUI-L008': 26,
    'KUI-L017': 1,
  },
};

function countsFor(diagnostics, severity) {
  const counts = {};
  for (const diagnostic of diagnostics) {
    if (diagnostic.severity !== severity) continue;
    counts[diagnostic.id] = (counts[diagnostic.id] ?? 0) + 1;
  }
  return counts;
}

export function assertKerfUiDoctorBaseline(report, budget = KERF_UI_DOCTOR_BUDGET) {
  if (report.schemaVersion !== 1) throw new Error(`Unsupported Kerf UI doctor report schema ${report.schemaVersion}.`);
  if (report.exitCode === 2 || report.exitCode === 130)
    throw new Error(`Kerf UI doctor did not complete successfully (exit ${report.exitCode}).`);

  const stages = new Map(report.stages.map((stage) => [stage.id, stage]));
  for (const id of ['catalog', 'typescript', 'eslint', 'analyzer']) {
    const status = stages.get(id)?.status;
    if (status !== 'ran' && status !== 'cached')
      throw new Error(`Kerf UI doctor stage ${id} was ${status ?? 'missing'}.`);
  }
  if (stages.get('browser')?.status !== 'skipped')
    throw new Error('The CI doctor gate must keep browser evaluation opt-in.');

  const failures = [];
  for (const severity of ['error', 'review']) {
    const actual = countsFor(report.diagnostics, severity);
    for (const [id, count] of Object.entries(actual)) {
      const limit = budget[severity][id] ?? 0;
      if (count > limit) failures.push(`${severity} ${id}: ${count} found, budget ${limit}`);
    }
  }
  if (failures.length) throw new Error(`Kerf UI doctor baseline regressed:\n${failures.join('\n')}`);

  return {
    errors: report.summary.errors,
    review: report.summary.review,
    warnings: report.summary.warnings,
    suppressed: report.summary.suppressed,
  };
}

function run() {
  const directory = dirname(fileURLToPath(import.meta.url));
  const workspace = resolve(directory, '..');
  const cli = resolve(workspace, 'node_modules/@kerfjs/ui/doctor/cli.mjs');
  const temporary = mkdtempSync(join(tmpdir(), 'hotsheet-kerf-doctor-'));
  const output = join(temporary, 'report.json');
  try {
    const result = spawnSync(process.execPath, [cli, '--full', '--format', 'json', '--output', output], {
      cwd: workspace,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    if (result.error) throw result.error;
    if (result.status !== 0 && result.status !== 1)
      throw new Error(result.stderr.trim() || `Kerf UI doctor exited ${result.status}.`);
    const summary = assertKerfUiDoctorBaseline(JSON.parse(readFileSync(output, 'utf8')));
    console.log(
      `Kerf UI doctor baseline accepted: ${summary.errors} errors, ${summary.review} review findings, ${summary.warnings} warnings, ${summary.suppressed} suppressed; browser evaluation skipped.`,
    );
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) run();
