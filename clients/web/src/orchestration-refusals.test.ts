import { describe, expect, it } from 'vitest';

import { ApiHttpError, parseAdmissionRefusal } from './api';
import { admissionRefusalText, describeRefusal, ORCHESTRATION_REFUSALS, refusalText } from './orchestration-refusals';

describe('HS2-8Z9VHH orchestration refusals', () => {
  it('words every refusal as a sentence with a next step only where a person can act', () => {
    for (const [code, text] of Object.entries(ORCHESTRATION_REFUSALS)) {
      expect(text.message, code).toMatch(/^[A-Z0-9].*\.$/);
      // A message is words, not the raw code (snake_case codes never appear in it).
      if (code.includes('_')) expect(text.message, code).not.toContain(code);
      if (text.next) expect(text.next, code).toMatch(/^[A-Z].*\.$/);
    }
    expect(refusalText(undefined)).toBeUndefined();
    expect(refusalText('no_such_code')).toBeUndefined();
  });

  it('states an admission refusal with its numbers and where to change the limit', () => {
    expect(
      admissionRefusalText({ reason: 'session_cap_reached', setting: 'max_sessions', limit: 2, observed: 2 }),
    ).toEqual({
      message: 'The machine already runs 2 AI sessions; its limit is 2.',
      next: 'Close a session, or raise Sessions per Machine in Settings › Orchestration › Machine Defaults.',
    });
    expect(admissionRefusalText({ reason: 'session_cap_reached', limit: 1, observed: 1 }).message).toContain(
      '1 AI session;',
    );
    expect(admissionRefusalText({ reason: 'session_cap_reached', limit: 0, observed: 0 }).message).toBe(
      'The machine’s session limit is 0, so no AI session may start.',
    );
    expect(admissionRefusalText({ reason: 'above_cpu_ceiling', limit: 90, observed: 97 }).message).toBe(
      'CPU load is 97%, above the 90% ceiling.',
    );
    expect(admissionRefusalText({ reason: 'below_disk_floor', limit: 5120, observed: 10 }).message).toBe(
      '10 MB of disk is free, below the 5120 MB floor.',
    );
    expect(admissionRefusalText({ reason: 'below_ram_floor', limit: 2048, observed: 100 }).message).toContain(
      '100 MB of memory is free',
    );
    expect(admissionRefusalText({ reason: 'tool_not_installed', tool: 'codex' }).message).toBe(
      'codex is not installed on that machine.',
    );
    expect(admissionRefusalText({ reason: 'tool_not_configured', tool: 'codex' }).message).toBe(
      'That project opted out of codex.',
    );
    // An older host sends only the reason; a newer one may send a reason this client lacks.
    expect(admissionRefusalText({ reason: 'session_cap_reached' })).toBe(ORCHESTRATION_REFUSALS.session_cap_reached);
    expect(admissionRefusalText({ reason: 'tool_not_installed' })).toBe(ORCHESTRATION_REFUSALS.tool_not_installed);
    expect(admissionRefusalText({ reason: 'brand_new' })).toBe(ORCHESTRATION_REFUSALS.session_admission_refused);
  });

  it('describes any failure in words, keeping the code as secondary detail and never alone', () => {
    const admission = parseAdmissionRefusal({ reason: 'session_cap_reached', limit: 2, observed: 3, extra: true });
    expect(admission).toEqual({
      reason: 'session_cap_reached',
      limit: 2,
      observed: 3,
      setting: undefined,
      tool: undefined,
    });
    const refused = new ApiHttpError('raw', 409, 'session_admission_refused', undefined, admission);
    expect(describeRefusal(refused, 'Propose Plan')).toBe(
      'Propose Plan: The machine already runs 3 AI sessions; its limit is 2. Close a session, or raise Sessions per Machine in Settings › Orchestration › Machine Defaults. (session_cap_reached)',
    );
    expect(describeRefusal(new ApiHttpError('raw', 409, 'run_paused'))).toBe(
      'That run is paused. Resume it first. (run_paused)',
    );
    expect(describeRefusal(new ApiHttpError('server words', 500, 'internal'))).toBe('server words');
    expect(describeRefusal(new Error('plain'))).toBe('plain');
    expect(describeRefusal('text')).toBe('text');
    for (const malformed of [undefined, null, 'x', {}, { reason: '' }, { reason: 4 }])
      expect(parseAdmissionRefusal(malformed)).toBeUndefined();
  });
});
