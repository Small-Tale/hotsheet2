import { mkdtempSync, rmSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  adaptFloatingToolbarComposition,
  assertKerfUiDoctorBaseline,
  KERF_UI_DOCTOR_BUDGET,
} from './check-kerf-ui-doctor.mjs';

function report(overrides = {}) {
  const diagnostics = Object.entries(KERF_UI_DOCTOR_BUDGET).flatMap(([severity, limits]) =>
    Object.entries(limits).flatMap(([id, count]) => Array.from({ length: count }, () => ({ id, severity }))),
  );
  return {
    schemaVersion: 1,
    exitCode: 1,
    stages: [
      { id: 'catalog', status: 'ran' },
      { id: 'typescript', status: 'ran' },
      { id: 'eslint', status: 'ran' },
      { id: 'analyzer', status: 'ran' },
      { id: 'browser', status: 'skipped' },
    ],
    diagnostics,
    summary: { errors: 199, review: 123, warnings: 1280, suppressed: 0 },
    ...overrides,
  };
}

describe('Kerf UI doctor baseline', () => {
  it('adapts only a cataloged group directly inside the documented floating toolbar', () => {
    const workspace = mkdtempSync(join(tmpdir(), 'hs-floating-toolbar-'));
    try {
      mkdirSync(join(workspace, 'src'));
      mkdirSync(join(workspace, 'node_modules/@kerfjs/ui'), { recursive: true });
      writeFileSync(join(workspace, 'node_modules/@kerfjs/ui/package.json'), '{"version":"5.0.0-beta.58"}');
      const lines = [
        "import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';",
        "import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';",
        'export function Example() {',
        '  return <section><FloatingToolbar label="Tools">',
        '    <ToolbarControlGroup><button /></ToolbarControlGroup>',
        '  </FloatingToolbar><div>',
        '    <ToolbarControlGroup><button /></ToolbarControlGroup>',
        '  </div></section>;',
        '}',
      ];
      writeFileSync(join(workspace, 'src/example.tsx'), lines.join('\n'));
      const diagnostic = (line) => ({
        id: 'KUI-L201',
        severity: 'error',
        stage: 'eslint',
        message: '`@kerfjs/ui:toolbar-control-group` requires one of these cataloged parents: @kerfjs/ui:toolbar.',
        location: { file: 'src/example.tsx', line, column: lines[line - 1].indexOf('<ToolbarControlGroup') + 1 },
      });
      const raw = report({ diagnostics: [diagnostic(5), diagnostic(7)], summary: { errors: 2 } });
      const adapted = adaptFloatingToolbarComposition(raw, workspace);
      expect(adapted.diagnostics).toEqual([diagnostic(7)]);
      expect(adapted.summary.errors).toBe(1);
      expect(adapted.floatingToolbarAdapted).toBe(1);
      expect(raw.diagnostics).toHaveLength(2);
      writeFileSync(join(workspace, 'node_modules/@kerfjs/ui/package.json'), '{"version":"5.0.0-beta.59"}');
      expect(adaptFloatingToolbarComposition(raw, workspace).diagnostics).toHaveLength(2);
    } finally {
      rmSync(workspace, { recursive: true, force: true });
    }
  });

  it('accepts the classified error and review budget with browser evaluation disabled', () => {
    expect(assertKerfUiDoctorBaseline(report())).toEqual({
      errors: 199,
      review: 123,
      warnings: 1280,
      suppressed: 0,
    });
  });

  it('accepts debt reduction but rejects a new diagnostic or a budget increase', () => {
    const reduced = report({ diagnostics: report().diagnostics.slice(1) });
    expect(() => assertKerfUiDoctorBaseline(reduced)).not.toThrow();
    const newDiagnostic = report({
      diagnostics: [...report().diagnostics, { id: 'KUI-L999', severity: 'error' }],
    });
    expect(() => assertKerfUiDoctorBaseline(newDiagnostic)).toThrow('error KUI-L999: 1 found, budget 0');
    const increased = report({
      diagnostics: [...report().diagnostics, { id: 'KUI-L004', severity: 'review' }],
    });
    expect(() => assertKerfUiDoctorBaseline(increased)).toThrow('review KUI-L004: 80 found, budget 79');
  });

  it('rejects failed stages, configuration failures, and an enabled browser stage', () => {
    expect(() => assertKerfUiDoctorBaseline(report({ exitCode: 2 }))).toThrow('did not complete successfully');
    expect(() =>
      assertKerfUiDoctorBaseline(
        report({
          stages: report().stages.map((stage) => (stage.id === 'eslint' ? { ...stage, status: 'failed' } : stage)),
        }),
      ),
    ).toThrow('stage eslint was failed');
    expect(() =>
      assertKerfUiDoctorBaseline(
        report({
          stages: report().stages.map((stage) => (stage.id === 'browser' ? { ...stage, status: 'ran' } : stage)),
        }),
      ),
    ).toThrow('browser evaluation opt-in');
  });
});
