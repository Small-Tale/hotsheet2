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

  it('replaces the activity spinner with a Kerf ETA ring and keeps the time label separate', () => {
    const indicator = String(
      ActiveClaimIndicator({
        label: 'Claude is actively working on this ticket',
        eta: { kind: 'estimate', percent: 40, label: '~25m left', title: 'Estimated to finish 10:00' },
      }),
    );
    expect(indicator).toContain('data-component="progress-ring"');
    expect(indicator).toContain('aria-valuenow="40"');
    expect(indicator).not.toContain('loading-spinner');
    const estimate = String(
      ClaimEta({ eta: { kind: 'estimate', percent: 40, label: '~25m left', title: 'Estimated to finish 10:00' } }),
    );
    expect(estimate).toContain('data-claim-eta="estimate"');
    expect(estimate).not.toContain('data-component="progress-ring"');
    expect(estimate).toContain('~25m left');
    const overrun = String(ClaimEta({ eta: { kind: 'overrun', label: 'Soon', title: 'Past its estimate' } }));
    expect(overrun).toContain('data-claim-eta="overrun"');
    expect(overrun).not.toContain('data-component="progress-ring"');
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
    expect(css).not.toContain('.claim-eta__ring');
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
    expect(withEta).toContain('<strong>Claude worker is working on this.</strong>');
    expect(withEta).not.toContain('loading-spinner');
    expect(withEta).toContain('data-component="progress-ring"');
    expect(withEta).toContain('data-claim-eta="estimate"');
    const withoutEta = String(LiveClaimNotice({ agentName: 'codex-01' }));
    expect(withoutEta).toContain('codex-01');
    expect(withoutEta).not.toContain('data-claim-eta');
    expect(withoutEta).toContain('var(--kui-state-banner-info-foreground)');
  });

  it('keeps a generated worker id accessible without letting it crowd the ETA', () => {
    const worker = 'codex-01M44ZE2BPEZKWTW49DCTATTPW';
    const markup = String(
      LiveClaimNotice({
        agentName: worker,
        eta: { kind: 'estimate', percent: 30, label: '1h 27m left', title: 'Estimated to finish later' },
      }),
    );
    expect(markup).toContain('<strong>Codex is working on this.</strong>');
    expect(markup).toContain(`title="${worker} is actively working on this ticket"`);
    expect(markup).toContain(`aria-label="${worker} is actively working on this ticket; 1h 27m left"`);
    expect(markup).toContain('1h 27m left');

    const custom = String(LiveClaimNotice({ agentName: 'verylongcustomworkeridentity' }));
    expect(custom).toContain('<strong>verylongcustomworke… is working on this.</strong>');
    expect(custom).toContain('title="verylongcustomworkeridentity is actively working on this ticket"');
  });

  it('styles only the notice gap; Kerf ProgressRing and Text own the ETA', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'active-claim.css'), 'utf8');
    expect(css).not.toContain('.claim-eta__ring');
    expect(css).toContain('.live-claim-notice {');
    expect(css.match(/^[^\s/*][^{]*\{/gm)).toHaveLength(1);
    expect(String(ClaimEta({ eta: { kind: 'estimate', label: '~45m left', title: 'ETA', percent: 40 } }))).toContain(
      'kui-text',
    );
    expect(css).not.toContain('.kui-state-banner');
    const rowCss = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    expect(rowCss).not.toContain('.ticket-list-row__eta');
  });
});
