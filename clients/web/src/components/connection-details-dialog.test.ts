import { describe, expect, it } from 'vitest';

import type { CompatibilityAssessment } from '../compatibility';
import { ConnectionDetailsDialog, connectionRecoveryGuidance } from './connection-details-dialog';

const stale: CompatibilityAssessment = {
  kind: 'compatible',
  revisionMismatch: true,
  sourceStale: true,
  canRestartServer: false,
  clientProtocol: { min: 1, max: 1 },
  clientRevision: 'source-sha256:client',
  server: {
    generation: 'hs2',
    application_version: '0.1.0',
    build_revision: 'source-sha256:old',
    source_revision: 'source-sha256:current',
    source_stale: true,
    protocol: { min: 1, max: 1 },
    started_at: '2026-09-02T08:00:00Z',
  },
};

describe('ConnectionDetailsDialog', () => {
  it('shows authoritative running/client metadata and accessible recovery guidance', () => {
    const markup = String(ConnectionDetailsDialog({ assessment: stale }));
    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-labelledby="connection-details-title"');
    expect(markup).toContain('aria-describedby="connection-details-summary"');
    expect(markup).toContain('Running server version');
    expect(markup).toContain('0.1.0');
    expect(markup).toContain('source-sha256:old');
    expect(markup).toContain('source-sha256:current');
    expect(markup).toContain('source-sha256:client');
    expect(markup).toContain('Client 1–1 · Server 1–1');
    expect(markup).toContain('popoverTargetAction="hide"');
    expect(connectionRecoveryGuidance(stale)).toContain('cargo build -p hotsheet-server');
  });

  it('does not imply an unsafe automatic restart when quiescence is unavailable', () => {
    const guidance = connectionRecoveryGuidance({ ...stale, kind: 'server_too_old', sourceStale: false, canRestartServer: false });
    expect(guidance).toContain('stop the old server manually after its active work finishes');
    expect(guidance).toContain('Automatic restart is unavailable');
  });
});
