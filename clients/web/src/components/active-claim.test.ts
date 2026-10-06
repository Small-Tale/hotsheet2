import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  ACTIVE_CLAIM_SPINNER_SIZE,
  ActiveClaimIndicator,
  ActiveClaimSpinner,
  ClaimEta,
  LiveClaimNotice,
} from './active-claim';

describe('active claim presentation (HS2-QKNQXC)', () => {
  it('renders a labeled spinner the row and header share', () => {
    const markup = String(ActiveClaimSpinner({ label: 'Codex is actively working on this ticket' }));
    expect(markup).toContain('Codex is actively working on this ticket');
    // Kerf sizes the spinner through LoadingSpinner `size`; the app no longer reaches into its svg (HS2-JVPPVV).
    expect(ACTIVE_CLAIM_SPINNER_SIZE).toBe(16.8);
    expect(markup).toMatch(/<svg[^>]*class="kui-loading-spinner"[^>]*data-size/);
    expect(markup).toContain('var(--hs-ticket-state-up-next)');
    const css = readFileSync(resolve(import.meta.dirname, 'active-claim.css'), 'utf8');
    expect(css).not.toContain('svg');
    expect(css).not.toContain('.active-claim-spinner');
  });

  it('replaces the activity spinner with a same-size ETA ring and keeps the time label separate', () => {
    const indicator = String(
      ActiveClaimIndicator({
        label: 'Claude is actively working on this ticket',
        eta: { kind: 'estimate', percent: 40, label: '~25m left', title: 'Estimated to finish 10:00' },
      }),
    );
    expect(indicator).toContain('<wa-progress-ring class="claim-eta__ring" value="40"');
    expect(indicator).not.toContain('loading-spinner');
    const estimate = String(
      ClaimEta({ eta: { kind: 'estimate', percent: 40, label: '~25m left', title: 'Estimated to finish 10:00' } }),
    );
    expect(estimate).toContain('data-claim-eta="estimate"');
    expect(estimate).not.toContain('wa-progress-ring');
    expect(estimate).toContain('~25m left');
    const overrun = String(ClaimEta({ eta: { kind: 'overrun', label: 'Soon', title: 'Past its estimate' } }));
    expect(overrun).toContain('data-claim-eta="overrun"');
    expect(overrun).not.toContain('wa-progress-ring');
    expect(overrun).toContain('Soon');
    expect(
      String(
        ActiveClaimIndicator({
          label: 'Claude is actively working on this ticket',
          eta: { kind: 'overrun', label: 'Soon', title: '' },
        }),
      ),
    ).toContain('data-component="loading-spinner"');
    const css = readFileSync(resolve(import.meta.dirname, 'active-claim.css'), 'utf8');
    expect(css).toContain('--size: remify(16.8px)');
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
    expect(withEta).toContain('data-component="state-banner"');
    expect(withEta).toContain('data-tone="info"');
    expect(withEta).toContain('<strong>Claude worker is working on this</strong>');
    expect(withEta).not.toContain('loading-spinner');
    expect(withEta).toContain('class="claim-eta__ring"');
    expect(withEta).toContain('data-claim-eta="estimate"');
    const withoutEta = String(LiveClaimNotice({ agentName: 'codex-01' }));
    expect(withoutEta).toContain('codex-01');
    expect(withoutEta).not.toContain('data-claim-eta');
    expect(withoutEta).toContain('var(--kui-state-banner-info-foreground)');
  });

  it('owns the spinner and ETA styles it renders', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'active-claim.css'), 'utf8');
    for (const selector of ['.claim-eta', '.claim-eta__ring', '.live-claim-notice'])
      expect(css).toContain(`${selector} {`);
    expect(css).not.toContain('.kui-state-banner');
    const rowCss = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    expect(rowCss).not.toContain('.ticket-list-row__eta');
  });
});
