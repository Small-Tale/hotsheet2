import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';
import './confidence-calibration.css';

import { type CssForegroundColor, foregroundColorVar } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { CircleAlert, Gauge } from 'lucide';

import type { CalibrationBand, CalibrationOutcome, CalibrationReport } from '../api';
import { ConfidenceBadge } from './confidence-badge';

/** What the calibration panel shows (HS2-Q1WCCY). */
export type ConfidenceCalibrationState =
  { status: 'loading' } | { status: 'error'; message: string } | { status: 'ready'; report: CalibrationReport };

const BAND_LABELS: Record<string, string> = {
  verified: 'Fully verified',
  assumed: 'Minor assumptions',
  partial: 'Partially verified',
  unverified: 'Largely unverified',
  unscored: 'Unscored',
};

const OUTCOME_LABELS: Record<CalibrationOutcome, string> = {
  verified: 'Verified',
  reopened: 'Reopened',
  pending: 'Awaiting outcome',
};

/** Each rubric band's label color, matching the ConfidenceBadge tones (HS2-PG7R4A). */
const BAND_COLORS: Record<string, CssForegroundColor | undefined> = {
  verified: foregroundColorVar('--wa-color-success-on-quiet'),
  assumed: foregroundColorVar('--wa-color-brand-on-quiet'),
  partial: foregroundColorVar('--wa-color-warning-on-quiet'),
  unverified: foregroundColorVar('--wa-color-danger-on-quiet'),
};

/** A resolved outcome's color; a pending one stays quiet. */
const OUTCOME_COLORS: Record<CalibrationOutcome, CssForegroundColor | undefined> = {
  verified: foregroundColorVar('--wa-color-success-on-quiet'),
  reopened: foregroundColorVar('--wa-color-danger-on-quiet'),
  pending: undefined,
};

/** A reopen rate as a whole percentage, or a dash until a completion resolves. */
export function reopenRateLabel(band: Pick<CalibrationBand, 'reopen_rate'>): string {
  return band.reopen_rate === undefined ? '—' : `${Math.round(band.reopen_rate * 100)}%`;
}

function BandRow({ band }: { band: CalibrationBand }) {
  const rate = band.reopen_rate ?? undefined;
  return (
    <tr data-band={band.band} data-key={`calibration-band:${band.band}`}>
      <th scope="row">
        <List>
          <Text variant="span" color={BAND_COLORS[band.band]} flush data-band={band.band}>
            <strong>{BAND_LABELS[band.band] ?? band.band}</strong>
          </Text>
          <Text variant="span" tone="quiet" size="compact" border="none" flush>
            {band.range === '-' ? 'no score' : band.range}
          </Text>
        </List>
      </th>
      <td>{band.completions}</td>
      <td>{band.verified}</td>
      <td>{band.reopened}</td>
      <td>{band.pending}</td>
      <td class="confidence-calibration__rate">
        <List gap="2xs">
          <span>{reopenRateLabel(band)}</span>
          {rate !== undefined && (
            <wa-progress-bar
              value={String(Math.round(rate * 100))}
              label={`${BAND_LABELS[band.band] ?? band.band} reopen rate`}
            />
          )}
        </List>
      </td>
      <td>{band.mean_confidence === undefined ? '—' : band.mean_confidence.toFixed(1)}</td>
    </tr>
  );
}

/**
 * Completion-confidence calibration for one project (HS2-Q1WCCY): per rubric band, how
 * many completions were later reopened versus verified, so the bands can be tuned. A
 * well-calibrated rubric reopens less as the band rises.
 */
export function ConfidenceCalibration({ state }: { state: ConfidenceCalibrationState }) {
  if (state.status === 'loading')
    return (
      <div class="confidence-calibration" data-component="confidence-calibration" data-state="loading">
        <EmptyState busy title="Loading confidence calibration" />
      </div>
    );
  if (state.status === 'error')
    return (
      <div class="confidence-calibration" data-component="confidence-calibration" data-state="error">
        <StateBanner
          title="Confidence calibration is unavailable"
          detail={state.message}
          tone="danger"
          urgency="alert"
          copyLayout="stacked"
          icon={<LucideIcon icon={CircleAlert} name="circle-alert" />}
        />
      </div>
    );
  const { report } = state;
  if (report.completions === 0)
    return (
      <div class="confidence-calibration" data-component="confidence-calibration" data-state="empty">
        <EmptyState
          title="No completions yet"
          detail="Calibration appears once tickets are completed. Each completion is compared with what happened next: reopened or verified."
          icon={<LucideIcon icon={Gauge} name="gauge" />}
        />
      </div>
    );
  const recent = [...report.events].reverse().slice(0, 8);
  return (
    <section
      class="confidence-calibration"
      data-component="confidence-calibration"
      data-state="ready"
      aria-labelledby="confidence-calibration-title"
    >
      <List gap="l">
        <List gap="xs">
          <Text variant="h2" size="large" flush id="confidence-calibration-title">
            Completion confidence calibration
          </Text>
          <Text variant="p" tone="quiet" flush>
            {report.completions} completions, {report.scored} scored. Reopen rate counts completions with a known
            outcome; a well-calibrated rubric reopens less as the band rises.
          </Text>
        </List>
        <div class="confidence-calibration__table-scroll">
          <table class="confidence-calibration__table">
            <caption class="confidence-calibration__caption">Reported confidence against later outcomes</caption>
            <thead>
              <tr>
                <th scope="col">Band</th>
                <th scope="col">Completions</th>
                <th scope="col">Verified</th>
                <th scope="col">Reopened</th>
                <th scope="col">Pending</th>
                <th scope="col">Reopen rate</th>
                <th scope="col">Mean score</th>
              </tr>
            </thead>
            <tbody>
              {report.bands.map((band) => (
                <BandRow band={band} />
              ))}
            </tbody>
          </table>
        </div>
        <section class="confidence-calibration__recent" aria-label="Recent completions">
          <List gap="xs">
            <ListHeader label="Recent completions" headingLevel={3} />
            <ul class="confidence-calibration__recent-list">
              {recent.map((event) => (
                <li data-key={`calibration-event:${event.slug}:${event.completed_at}`} data-outcome={event.outcome}>
                  <Row wrap gap="xs" vAlign="middle" textInsets="l">
                    <Text variant="span" font="monospace" size="compact" border="none" flush>
                      {event.slug}
                    </Text>
                    {event.confidence === undefined ? (
                      <Text variant="span" tone="quiet" size="compact" border="none" flush>
                        Unscored
                      </Text>
                    ) : (
                      <ConfidenceBadge value={event.confidence} />
                    )}
                    <Text
                      variant="span"
                      tone="quiet"
                      size="compact"
                      border="none"
                      flush
                      color={OUTCOME_COLORS[event.outcome]}
                      data-outcome={event.outcome}
                    >
                      {OUTCOME_LABELS[event.outcome]}
                    </Text>
                  </Row>
                </li>
              ))}
            </ul>
          </List>
        </section>
      </List>
    </section>
  );
}
