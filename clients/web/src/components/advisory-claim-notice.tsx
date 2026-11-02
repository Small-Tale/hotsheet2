import { Button } from '@kerfjs/ui/button';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';

import { advisoryRenewExpiry } from '../advisory-claim-resource';
import type { AdvisoryClaimStatus } from '../api';

export interface AdvisoryClaimNoticeProps {
  status?: AdvisoryClaimStatus;
  loading: boolean;
  busy: boolean;
  error?: string;
  worker: string;
  operationId?: string;
}

/** External process intent is advisory; this notice never presents a lock or owner proof. */
export function AdvisoryClaimNotice({ status, loading, busy, error, worker, operationId }: AdvisoryClaimNoticeProps) {
  const own = status?.projection.claims.some((claim) => claim.worker === worker && claim.operation_id === operationId);
  const needsAttention = status?.pending_operations.some((operation) => operation.state === 'needs_attention');
  const claimNames = status?.projection.claims
    .map((claim) => claim.worker)
    .slice(0, 3)
    .join(', ');
  const title = error
    ? 'Advisory claim state unavailable'
    : loading
      ? 'Checking advisory claims'
      : status?.projection.conflicted
        ? 'Overlapping advisory claims'
        : needsAttention
          ? 'Advisory claim needs review'
          : status?.provisional
            ? 'Provisional advisory claim pending'
            : status?.projection.claims.length
              ? own
                ? 'Your advisory claim is active'
                : 'Another process has an advisory claim'
              : 'No advisory process claim';
  const detail = error
    ? `Remote claim state could not be verified. Automatic handoff is paused. ${error}`
    : loading
      ? 'Reading provider markers and local pending work.'
      : [
          status?.projection.conflicted
            ? `Conflicting workers: ${claimNames || 'invalid provider markers'}${status.projection.claims.length > 3 ? ', …' : ''}.`
            : undefined,
          needsAttention ? 'The local claim needs review.' : undefined,
          status?.provisional ? 'Local intent is pending provider confirmation.' : undefined,
          !status?.projection.conflicted && !status?.provisional && status?.projection.claims.length
            ? `Worker: ${claimNames}. This marker is best effort and does not exclude other workers.`
            : undefined,
          status && !status.automatic_handoff_safe ? 'Automatic handoff is paused.' : undefined,
          status && !status.projection.claims.length && !status.provisional && !needsAttention
            ? 'A process can record best-effort intent before working on this ticket.'
            : undefined,
        ]
          .filter(Boolean)
          .join(' ');
  const tone =
    error || needsAttention || status?.projection.conflicted ? 'warning' : status?.provisional ? 'warning' : 'info';
  const canClaim =
    !loading &&
    !busy &&
    !error &&
    status?.remote_verified &&
    !status.provisional &&
    !status.projection.conflicted &&
    status.projection.claims.length === 0;
  const canRenew =
    !loading &&
    !busy &&
    !error &&
    own &&
    Boolean(operationId && advisoryRenewExpiry(operationId) !== undefined) &&
    !status?.provisional &&
    !status?.projection.conflicted;
  const canRelease = !loading && !busy && !error && own && !status?.provisional;
  return (
    <div data-component="advisory-claim-notice">
      <StateBanner
        title={title}
        detail={detail}
        tone={tone}
        urgency="status"
        copyLayout="stacked"
        actionPlacement="below"
        action={
          !loading ? (
            <Row wrap gap="xs">
              {canClaim && (
                <Button type="button" emphasis="secondary" action="advisory-claim">
                  Claim Advisory
                </Button>
              )}
              {canRenew && (
                <Button type="button" emphasis="secondary" action="advisory-renew">
                  Renew Advisory
                </Button>
              )}
              {canRelease && (
                <Button type="button" emphasis="secondary" action="advisory-release">
                  Release Advisory
                </Button>
              )}
              <Button type="button" emphasis="secondary" action="advisory-refresh" disabled={busy}>
                Refresh
              </Button>
            </Row>
          ) : undefined
        }
      />
    </div>
  );
}
