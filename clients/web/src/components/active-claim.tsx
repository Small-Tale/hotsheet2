import './active-claim.css';

import { ProgressRing } from '@kerfjs/ui/progress-ring';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';

import type { ClaimEtaPresentation } from '../active-ticket-work';

/**
 * The live-work symbol the ticket row and the inspector/reader header share (HS2-XQMDQB, HS2-QKNQXC): one
 * small Kerf ProgressRing, determinate while an ETA is on track and indeterminate otherwise. A live claim
 * is long work of unknown length, which Kerf assigns to an indeterminate ProgressRing rather than to
 * LoadingSpinner's brief busy glyph. The ring also spins its whole SVG box, which WebKit composites;
 * LoadingSpinner spins a shape inside its SVG and repainted every frame for as long as a claim was live,
 * keeping an idle Safari web app at 20% CPU or more per spinner (HS2-GFZYCZ). Chromium still runs the ring's
 * `rotate` animation on the main thread until Kerf animates `transform` instead (KF-M4WWRW).
 */
export function ActiveClaimIndicator({ label, eta }: { label: string; eta?: ClaimEtaPresentation }) {
  return eta?.kind === 'estimate' ? (
    <ProgressRing value={eta.percent} max={100} size="small" label={`${label}; ${eta.label}`} />
  ) : (
    <ProgressRing size="small" label={label} />
  );
}

/** The time-left label remains beside the holder when the ring moves to the activity slot. */
export function ClaimEta({ eta }: { eta: ClaimEtaPresentation }) {
  return (
    <Text variant="span" tone="quiet" size="compact" wrap="nowrap" data-claim-eta={eta.kind} title={eta.title}>
      {eta.label}
    </Text>
  );
}

export interface LiveClaimNoticeProps {
  /** Who holds the live claim (worker label, else worker id). */
  agentName: string;
  eta?: ClaimEtaPresentation;
  /**
   * HS2-ZR8B4N: `provisional` for a claim taken while the claim backend was unreachable, not confirmed and never
   * exclusive; `lost` for a claim this machine's `agentName` held that another machine now holds (`holder`).
   */
  standing?: { kind: 'provisional' } | { kind: 'lost'; holder: string };
}

function visibleClaimHolder(agentName: string): string {
  const generated = /^(codex|claude|opencode|antigravity)-[0-9A-HJKMNP-TV-Z]{26}$/i.exec(agentName);
  if (generated) {
    const names: Record<string, string> = {
      codex: 'Codex',
      claude: 'Claude',
      opencode: 'OpenCode',
      antigravity: 'Antigravity',
    };
    return names[generated[1].toLowerCase()];
  }
  const characters = Array.from(agentName);
  return characters.length > 22 ? `${characters.slice(0, 19).join('')}…` : agentName;
}

/** Header status line for a ticket someone is actively working on (HS2-QKNQXC). */
export function LiveClaimNotice({ agentName, eta, standing }: LiveClaimNoticeProps) {
  // HS2-ZR8B4N: a lost claim names who lost it and who holds it now; nobody here works on it.
  if (standing?.kind === 'lost') {
    const label = `${agentName} lost the claim on this ticket to ${standing.holder}`;
    return (
      <div class="live-claim-notice" data-component="live-claim-notice" data-claim-standing="lost" title={label}>
        <StateBanner
          title={`${visibleClaimHolder(agentName)} lost this claim.`}
          detail={`Another machine holds it now (${standing.holder}).`}
          copyLayout="stacked"
          tone="danger"
          urgency="status"
        />
      </div>
    );
  }
  // HS2-ZR8B4N: a provisional claim never reads as exclusive work.
  if (standing?.kind === 'provisional') {
    const label = `${agentName} holds a provisional claim on this ticket`;
    return (
      <div class="live-claim-notice" data-component="live-claim-notice" data-claim-standing="provisional" title={label}>
        <StateBanner
          title={`${visibleClaimHolder(agentName)} is working on this provisionally.`}
          detail="Not confirmed yet; another machine may hold it."
          tone="warning"
          urgency="status"
          icon={<ActiveClaimIndicator label={label} eta={eta} />}
          action={eta ? <ClaimEta eta={eta} /> : undefined}
        />
      </div>
    );
  }
  const label = `${agentName} is actively working on this ticket`;
  return (
    <div class="live-claim-notice" data-component="live-claim-notice" title={label}>
      <StateBanner
        title={`${visibleClaimHolder(agentName)} is working on this.`}
        tone="info"
        urgency="status"
        icon={<ActiveClaimIndicator label={label} eta={eta} />}
        action={eta ? <ClaimEta eta={eta} /> : undefined}
      />
    </div>
  );
}
