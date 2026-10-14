import './phase-estimate.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Timer } from 'lucide';

import type { PhaseEstimatePresentation } from '../active-ticket-work';

/**
 * Estimated time left in a testing phase, shown beside the phase chip on ticket rows, column cards,
 * and the detail status line (HS2-YDGM04). "Soon" once the phase runs past its typical duration.
 */
export function PhaseEstimate({
  estimate,
  compact = false,
}: {
  estimate: PhaseEstimatePresentation;
  compact?: boolean;
}) {
  return (
    <span
      class="phase-estimate"
      data-component="phase-estimate"
      data-phase-estimate={estimate.kind}
      title={estimate.title}
    >
      <Row gap="2xs" vAlign="middle" wrap={false}>
        <LucideIcon icon={Timer} name="timer" size={compact ? 'xs' : 's'} />
        <Text variant="span" tone="quiet" size="compact" wrap="nowrap">
          {estimate.label}
        </Text>
      </Row>
    </span>
  );
}
