import { Badge } from '@kerfjs/ui/badge';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Gauge } from 'lucide';

export type ConfidenceBand = 'verified' | 'assumed' | 'partial' | 'unverified';

/** Map a 0-100 score to the documented anchor bands (docs/05 completion confidence rubric). */
export function confidenceBand(value: number): ConfidenceBand {
  if (value >= 90) return 'verified';
  if (value >= 70) return 'assumed';
  if (value >= 40) return 'partial';
  return 'unverified';
}

const confidenceBandLabels: Record<ConfidenceBand, string> = {
  verified: 'fully verified',
  assumed: 'verified with minor assumptions',
  partial: 'partially verified',
  unverified: 'largely unverified',
};

/** Each rubric band's semantic Kerf Badge tone. */
export const CONFIDENCE_BAND_TONES = {
  verified: 'success',
  assumed: 'info',
  partial: 'warning',
  unverified: 'danger',
} as const satisfies Record<ConfidenceBand, string>;

/**
 * AI completion confidence badge (HS2-DWTJ43), tinted by its rubric band. `compact` (gauge
 * icon + percentage) is the pill used on note cards and ticket list/board summaries
 * (HS2-A0Q6G6); `labeled` ("Confidence NN%") is reserved for the inspector/reader header.
 *
 * Kerf's `Badge` owns the pill, its band tone, and both sizes, so there is no stylesheet
 * (HS2-MBFKJF). The bare outer span only carries the band identity, accessible name, and
 * explanatory title, which Badge cannot take yet (KF-2YKKK5).
 */
export function ConfidenceBadge({
  value,
  appearance = 'compact',
}: {
  value: number;
  appearance?: 'compact' | 'labeled';
}) {
  const band = confidenceBand(value);
  const labeled = appearance === 'labeled';
  return (
    <span
      class="confidence-badge"
      data-component="confidence-badge"
      data-appearance={appearance}
      data-band={band}
      data-confidence={value}
      role="img"
      aria-label={`Confidence ${value} percent`}
      title={`AI-reported confidence ${value}%: ${confidenceBandLabels[band]}`}
    >
      <Badge tone={CONFIDENCE_BAND_TONES[band]} size={labeled ? 'default' : 'compact'} ariaHidden>
        <Row gap="2xs" vAlign="middle">
          <LucideIcon icon={Gauge} name="gauge" size={labeled ? 's' : 'xs'} />
          <span>{labeled ? `Confidence ${value}%` : `${value}%`}</span>
        </Row>
      </Badge>
    </span>
  );
}
