import '@awesome.me/webawesome/dist/components/progress-ring/progress-ring.js';
import './active-claim.css';

import { type CssForegroundColor, foregroundColorVar } from '@kerfjs/ui/css-values';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { StateBanner } from '@kerfjs/ui/state-banner';

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

/** The live-work symbol is determinate while an ETA is on track and indeterminate otherwise. */
export function ActiveClaimIndicator({
  label,
  eta,
  color,
}: {
  label: string;
  eta?: ClaimEtaPresentation;
  color?: CssForegroundColor;
}) {
  return eta?.kind === 'estimate' ? (
    <wa-progress-ring class="claim-eta__ring" value={eta.percent} aria-label={`${label}; ${eta.label}`} />
  ) : (
    <ActiveClaimSpinner label={label} color={color} />
  );
}

/** The time-left label remains beside the holder when the ring moves to the activity slot. */
export function ClaimEta({ eta }: { eta: ClaimEtaPresentation }) {
  return (
    <span class="claim-eta" data-claim-eta={eta.kind} title={eta.title}>
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
    <div class="live-claim-notice" data-component="live-claim-notice" title={label}>
      <StateBanner
        title={`${agentName} is working on this`}
        tone="info"
        urgency="status"
        icon={
          <ActiveClaimIndicator
            label={label}
            eta={eta}
            color={foregroundColorVar('--kui-state-banner-info-foreground')}
          />
        }
        action={eta ? <ClaimEta eta={eta} /> : undefined}
      />
    </div>
  );
}
