import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { ActiveClaimSpinner, ClaimEta, LiveClaimNotice } from './active-claim';

describe('active claim presentation (HS2-QKNQXC)', () => {
  it('renders a labeled spinner the row and header share', () => {
    const markup = String(ActiveClaimSpinner({ label: 'Codex is actively working on this ticket' }));
    expect(markup).toContain('class="active-claim-spinner"');
    expect(markup).toContain('Codex is actively working on this ticket');
  });

  it('renders the ETA ring only for an estimate and the overrun as Soon', () => {
    const estimate = String(
      ClaimEta({ eta: { kind: 'estimate', percent: 40, label: '~25m left', title: 'Estimated to finish 10:00' } }),
    );
    expect(estimate).toContain('data-claim-eta="estimate"');
    expect(estimate).toContain('<wa-progress-ring class="claim-eta__ring" value="40"');
    expect(estimate).toContain('~25m left');
    const overrun = String(ClaimEta({ eta: { kind: 'overrun', label: 'Soon', title: 'Past its estimate' } }));
    expect(overrun).toContain('data-claim-eta="overrun"');
    expect(overrun).not.toContain('wa-progress-ring');
    expect(overrun).toContain('Soon');
  });

  it('names the claim holder and shows the ETA only when one exists', () => {
    const withEta = String(
      LiveClaimNotice({
        agentName: 'Claude worker',
        eta: { kind: 'estimate', percent: 10, label: '~1h left', title: 'Estimated to finish 11:00' },
      }),
    );
    expect(withEta).toContain('data-component="live-claim-notice"');
    expect(withEta).toContain('role="status"');
    expect(withEta).toContain('<span class="live-claim-notice__agent">Claude worker</span> is working on this');
    expect(withEta).toContain('class="active-claim-spinner"');
    expect(withEta).toContain('data-claim-eta="estimate"');
    const withoutEta = String(LiveClaimNotice({ agentName: 'codex-01' }));
    expect(withoutEta).toContain('codex-01');
    expect(withoutEta).not.toContain('data-claim-eta');
  });

  it('owns the spinner and ETA styles it renders', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'active-claim.css'), 'utf8');
    for (const selector of ['.active-claim-spinner', '.claim-eta', '.claim-eta__ring', '.live-claim-notice'])
      expect(css).toContain(`${selector} {`);
    const rowCss = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    expect(rowCss).not.toContain('.ticket-list-row__eta');
  });
});
