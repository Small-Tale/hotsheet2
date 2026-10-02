import { describe, expect, it } from 'vitest';

import { assertKerfUiDoctorClean } from './check-kerf-ui-doctor.mjs';

const report = (overrides = {}) => ({
  schemaVersion: 1,
  exitCode: 0,
  diagnostics: [],
  // Matching the real report: documented suppressions move out of `diagnostics` into `suppressions`.
  suppressions: [{ id: 'KUI-L401', severity: 'warning', stage: 'eslint', message: 'wiring' }],
  summary: { errors: 0, review: 0, warnings: 0, suppressed: 1 },
  ...overrides,
});

describe('Kerf UI doctor gate', () => {
  it('accepts a report whose only diagnostics are documented suppressions', () => {
    expect(assertKerfUiDoctorClean(report())).toEqual({ errors: 0, review: 0, warnings: 0, suppressed: 1 });
  });

  it('fails on any active error, review finding, or warning', () => {
    for (const severity of ['error', 'review', 'warning'])
      expect(() =>
        assertKerfUiDoctorClean(report({ diagnostics: [{ id: 'KUI-L999', severity, message: 'new finding' }] })),
      ).toThrow(`${severity} KUI-L999: new finding`);
  });

  it('fails when the doctor could not complete', () => {
    expect(() => assertKerfUiDoctorClean(report({ exitCode: 2 }))).toThrow('did not complete successfully');
    expect(() => assertKerfUiDoctorClean(report({ schemaVersion: 2 }))).toThrow('Unsupported');
  });
});
