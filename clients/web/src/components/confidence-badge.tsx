import { Badge } from '@kerfjs/ui/badge';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
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
 * Kerf's `Badge` is the root and there is no stylesheet (HS2-MBFKJF, HS2-PQHN1J): it owns the
 * pill, band tone, both sizes, the leading icon, the explanatory `title`, the labeled-image
 * accessible name, and the band identity in `rootAttributes` (KF-2YKKK5). Select the badge by
 * `data-confidence-badge` (its appearance); Badge protects `data-component`.
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
    <Badge
      tone={CONFIDENCE_BAND_TONES[band]}
      size={labeled ? 'default' : 'compact'}
      icon={<LucideIcon icon={Gauge} name="gauge" />}
      label={`Confidence ${value} percent`}
      title={`AI-reported confidence ${value}%: ${confidenceBandLabels[band]}`}
      rootAttributes={{
        'data-confidence-badge': appearance,
        'data-band': band,
        'data-confidence': String(value),
      }}
    >
      {labeled ? `Confidence ${value}%` : `${value}%`}
    </Badge>
  );
}
