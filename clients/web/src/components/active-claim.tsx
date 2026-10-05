import '@awesome.me/webawesome/dist/components/progress-ring/progress-ring.js';
import './active-claim.css';

import { type CssForegroundColor, foregroundColorVar, uiColor } from '@kerfjs/ui/css-values';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';

import type { ClaimEtaPresentation } from '../active-ticket-work';

/** The live-claim spinner's pixel size, converted to rem by Kerf. */
export const ACTIVE_CLAIM_SPINNER_SIZE = 16.8;

/**
 * The live-claim activity spinner shared by ticket rows and the inspector/reader header (HS2-XQMDQB,
 * HS2-QKNQXC). Kerf owns its size and color through LoadingSpinner props.
 */
export function ActiveClaimSpinner({
  label,
  color = foregroundColorVar('--hs-ticket-state-up-next'),
}: {
  label: string;
  color?: CssForegroundColor;
}) {
  return <LoadingSpinner label={label} size={ACTIVE_CLAIM_SPINNER_SIZE} color={color} />;
}

/** Progress toward a live claim's ETA: a determinate ring plus time left, or "Soon" once overrun. */
export function ClaimEta({ eta }: { eta: ClaimEtaPresentation }) {
  return (
    <span class="claim-eta" data-claim-eta={eta.kind} title={eta.title}>
      {eta.kind === 'estimate' && <wa-progress-ring class="claim-eta__ring" value={eta.percent} aria-hidden="true" />}
      <span class="claim-eta__label">{eta.label}</span>
    </span>
  );
}

export interface LiveClaimNoticeProps {
  /** Who holds the live claim (worker label, else worker id). */
  agentName: string;
  eta?: ClaimEtaPresentation;
}

/** Header status line for a ticket someone is actively working on (HS2-QKNQXC). */
export function LiveClaimNotice({ agentName, eta }: LiveClaimNoticeProps) {
  const label = `${agentName} is actively working on this ticket`;
  return (
    <div class="live-claim-notice" role="status" data-component="live-claim-notice" title={label}>
      <ActiveClaimSpinner label={label} color={uiColor('brand-on-quiet')} />
      <span class="live-claim-notice__text">
        <span class="live-claim-notice__agent">{agentName}</span> is working on this
      </span>
      {eta && <ClaimEta eta={eta} />}
    </div>
  );
}
