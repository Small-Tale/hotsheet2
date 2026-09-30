import { readFileSync, rmSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import ts from 'typescript';

export const KERF_UI_DOCTOR_BUDGET = {
  error: {
    'KUI-L001': 2,
    'KUI-L011': 0,
    'KUI-L017': 0,
    'KUI-L019': 83,
    'KUI-L020': 0,
    'KUI-L022': 33,
    'KUI-L101': 0,
    'KUI-L102': 0,
    'KUI-L103': 0,
    'KUI-L201': 0,
    'KUI-L202': 0,
    'KUI-L203': 0,
  },
  review: {
    'KUI-L004': 0,
    'KUI-L006': 0,
    'KUI-L008': 0,
    'KUI-L017': 0,
  },
};

function importedNames(source, subpath, exportedName) {
  const names = new Set();
  for (const statement of source.statements) {
    if (!ts.isImportDeclaration(statement) || statement.moduleSpecifier.text !== subpath) continue;
    const bindings = statement.importClause?.namedBindings;
    if (!bindings || !ts.isNamedImports(bindings)) continue;
    for (const element of bindings.elements)
      if ((element.propertyName?.text ?? element.name.text) === exportedName) names.add(element.name.text);
  }
  return names;
}

function isFloatingToolbarChild(source, line, column) {
  const groupNames = importedNames(source, '@kerfjs/ui/toolbar-control-group', 'ToolbarControlGroup');
  const floatingNames = importedNames(source, '@kerfjs/ui/floating-toolbar', 'FloatingToolbar');
  if (!groupNames.size || !floatingNames.size) return false;
  const position = source.getPositionOfLineAndCharacter(line - 1, column - 1);
  let opening;
  const visit = (node) => {
    if (node.getStart(source) > position || node.end <= position) return;
    if (ts.isJsxOpeningElement(node)) opening = node;
    ts.forEachChild(node, visit);
  };
  visit(source);
  if (!opening || !groupNames.has(opening.tagName.getText(source))) return false;
  const parent = opening.parent?.parent;
  return Boolean(parent && ts.isJsxElement(parent) && floatingNames.has(parent.openingElement.tagName.getText(source)));
}

/** Kerf versions whose composition catalog still omits the documented FloatingToolbar →
 * ToolbarControlGroup parent edge (HS2-10KEHN). Add a version only after confirming the gap. */
const FLOATING_TOOLBAR_GAP_VERSIONS = new Set(['5.0.0-beta.58', '5.0.0-beta.59']);

/** Kerf beta.58/59 document ToolbarControlGroup as FloatingToolbar children, but their
 * composition catalog lists only Toolbar as a parent. Apply that exact missing parent
 * edge locally until the upstream catalog includes it; every other L201 remains gated. */
export function adaptFloatingToolbarComposition(report, workspace) {
  const kerfVersion = JSON.parse(
    readFileSync(resolve(workspace, 'node_modules/@kerfjs/ui/package.json'), 'utf8'),
  ).version;
  if (!FLOATING_TOOLBAR_GAP_VERSIONS.has(kerfVersion)) return { ...report, floatingToolbarAdapted: 0 };
  const sources = new Map();
  let adapted = 0;
  const diagnostics = report.diagnostics.filter((diagnostic) => {
    const { file, line, column } = diagnostic.location ?? {};
    if (
      diagnostic.id !== 'KUI-L201' ||
      diagnostic.stage !== 'eslint' ||
      diagnostic.severity !== 'error' ||
      !diagnostic.message?.includes('`@kerfjs/ui:toolbar-control-group` requires one of these cataloged parents') ||
      !file?.startsWith('src/') ||
      file.split('/').includes('..') ||
      !Number.isInteger(line) ||
      !Number.isInteger(column)
    )
      return true;
    let source = sources.get(file);
    if (!source) {
      const text = readFileSync(resolve(workspace, file), 'utf8');
      source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
      sources.set(file, source);
    }
    if (!isFloatingToolbarChild(source, line, column)) return true;
    adapted += 1;
    return false;
  });
  return {
    ...report,
    diagnostics,
    summary: { ...report.summary, errors: report.summary.errors - adapted },
    floatingToolbarAdapted: adapted,
  };
}

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
    const report = adaptFloatingToolbarComposition(JSON.parse(readFileSync(output, 'utf8')), workspace);
    const summary = assertKerfUiDoctorBaseline(report);
    console.log(
      `Kerf UI doctor baseline accepted: ${summary.errors} errors, ${summary.review} review findings, ${summary.warnings} warnings, ${summary.suppressed} suppressed; ${report.floatingToolbarAdapted} documented FloatingToolbar parent findings adapted; browser evaluation skipped.`,
    );
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) run();
