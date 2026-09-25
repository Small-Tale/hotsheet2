import { describe, expect, it } from 'vitest';

import { assertKerfUiDoctorBaseline, KERF_UI_DOCTOR_BUDGET } from './check-kerf-ui-doctor.mjs';

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
    summary: { errors: 40, review: 177, warnings: 1128, suppressed: 0 },
    ...overrides,
  };
}

describe('Kerf UI doctor baseline', () => {
  it('accepts the classified error and review budget with browser evaluation disabled', () => {
    expect(assertKerfUiDoctorBaseline(report())).toEqual({
      errors: 40,
      review: 177,
      warnings: 1128,
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
    expect(() => assertKerfUiDoctorBaseline(increased)).toThrow('review KUI-L004: 82 found, budget 81');
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
