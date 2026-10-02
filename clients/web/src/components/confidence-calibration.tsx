import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';
import './confidence-calibration.css';

import { EmptyState } from '@kerfjs/ui/empty-state';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
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

/** A reopen rate as a whole percentage, or a dash until a completion resolves. */
export function reopenRateLabel(band: Pick<CalibrationBand, 'reopen_rate'>): string {
  return band.reopen_rate === undefined ? '—' : `${Math.round(band.reopen_rate * 100)}%`;
}

function BandRow({ band }: { band: CalibrationBand }) {
  const rate = band.reopen_rate ?? undefined;
  return (
    <tr data-band={band.band} data-key={`calibration-band:${band.band}`}>
      <th scope="row">
        <span class="confidence-calibration__band" data-band={band.band}>
          {BAND_LABELS[band.band] ?? band.band}
        </span>
        <span class="confidence-calibration__range">{band.range === '-' ? 'no score' : band.range}</span>
      </th>
      <td>{band.completions}</td>
      <td>{band.verified}</td>
      <td>{band.reopened}</td>
      <td>{band.pending}</td>
      <td class="confidence-calibration__rate">
        <div class="confidence-calibration__rate-stack">
          <span>{reopenRateLabel(band)}</span>
          {rate !== undefined && (
            <wa-progress-bar
              value={String(Math.round(rate * 100))}
              label={`${BAND_LABELS[band.band] ?? band.band} reopen rate`}
            ></wa-progress-bar>
          )}
        </div>
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
      <header class="confidence-calibration__header">
        <h2 id="confidence-calibration-title">Completion confidence calibration</h2>
        <p>
          {report.completions} completions, {report.scored} scored. Reopen rate counts completions with a known outcome;
          a well-calibrated rubric reopens less as the band rises.
        </p>
      </header>
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
      <section class="confidence-calibration__recent" aria-labelledby="confidence-calibration-recent">
        <h3 class="confidence-calibration__recent-title" id="confidence-calibration-recent">
          Recent completions
        </h3>
        <ul class="confidence-calibration__recent-list">
          {recent.map((event) => (
            <li
              class="confidence-calibration__recent-item"
              data-key={`calibration-event:${event.slug}:${event.completed_at}`}
              data-outcome={event.outcome}
            >
              <span class="confidence-calibration__slug">{event.slug}</span>
              {event.confidence === undefined ? (
                <span class="confidence-calibration__unscored">Unscored</span>
              ) : (
                <ConfidenceBadge value={event.confidence} />
              )}
              <span class="confidence-calibration__outcome" data-outcome={event.outcome}>
                {OUTCOME_LABELS[event.outcome]}
              </span>
            </li>
          ))}
        </ul>
      </section>
    </section>
  );
}
