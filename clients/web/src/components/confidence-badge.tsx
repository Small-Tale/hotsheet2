import './confidence-badge.css';

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

/**
 * AI completion confidence badge (HS2-DWTJ43), tinted by its rubric band. `compact` (gauge
 * icon + percentage) is the pill used on note cards and ticket list/board summaries
 * (HS2-A0Q6G6); `labeled` ("Confidence NN%") is reserved for the inspector/reader header.
 */
export function ConfidenceBadge({
  value,
  appearance = 'compact',
}: {
  value: number;
  appearance?: 'compact' | 'labeled';
}) {
  const band = confidenceBand(value);
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
      <LucideIcon icon={Gauge} name="gauge" />
      <span aria-hidden="true">{appearance === 'labeled' ? `Confidence ${value}%` : `${value}%`}</span>
    </span>
  );
}
