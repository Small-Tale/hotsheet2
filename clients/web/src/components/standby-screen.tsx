import './standby-screen.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';

import { LIVE_STATE_ICONS } from '../live-state-icons';
import type { StandbyModel } from '../standby';
import { TOP_LAYER_OVERLAY_ATTRIBUTE } from '../top-layer-overlay';
import { GlanceGauge } from './glance-gauge';

export interface StandbyScreenProps {
  /** The clock face, already formatted (`2:47`). */
  clock: string;
  model: StandbyModel;
  /** The freshness line; `live: false` says the change stream dropped, so it brightens. */
  freshness: { live: boolean; label: string };
  /**
   * `overlay` (the app) covers the viewport as a top-layer popover; `inline` (the UX catalog) fills
   * its container so the display can be staged at any size.
   */
  presentation?: 'overlay' | 'inline';
  /** Respect a phone or tablet hint: say "tap" instead of "click". */
  touch?: boolean;
}

/** Average advance of a light tabular digit, in em; the colon is narrower. */
const DIGIT_EM = 0.58;

/**
 * A screen-sized numeral (the clock and the count): SVG text in its own view box, so CSS sizes it
 * by height from the screen like the gauge, beyond the typography scale's largest step.
 */
function numeral(text: string, label: string) {
  // Digits and separators only: the text is a clock face or a count.
  const colons = text.split(':').length - 1;
  const width = Math.max(1, (text.length - colons) * DIGIT_EM + colons * 0.3) * 100;
  return (
    <svg class="standby-screen__numeral" viewBox={`0 12 ${width} 76`} role="img" aria-label={`${label}: ${text}`}>
      <text x={String(width / 2)} y="86" text-anchor="middle" font-size="100">
        {text}
      </text>
    </svg>
  );
}

/**
 * The standby night display (HS2-EQVZTZ, mocks `m4`–`m6` on HS2-CT5EWR, docs/34-glance-surfaces.md
 * §34.5): solid black, a very large dark-red tokens/min gauge and the count of tickets updated in the
 * last 60 minutes, a clock, a dim per-project strip, and a freshness line. Only problems brighten: an
 * amber callout and that project's row. Kerf has no ambient display, so this is an app-owned domain
 * visualization like the glance gauge it hosts; the whole surface is the exit target, and the owner
 * exits on any click, tap, or key.
 */
export function StandbyScreen({
  clock,
  model,
  freshness,
  presentation = 'overlay',
  touch = false,
}: StandbyScreenProps) {
  const overlay = presentation === 'overlay';
  const content = (
    <div class="standby-screen__content">
      <header class="standby-screen__top">
        <div class="standby-screen__clock-group">
          <time class="standby-screen__clock" data-role="standby-clock">
            {numeral(clock, 'Time')}
          </time>
          <span class="standby-screen__scope" data-role="standby-scope">
            {model.scopeLabel}
          </span>
        </div>
        <span class="standby-screen__hint">{touch ? 'Tap anywhere to exit' : 'Click or press any key to exit'}</span>
      </header>
      <div class="standby-screen__hero">
        <div class="standby-screen__gauge">
          <GlanceGauge
            value={model.tokensPerMinute}
            peak={model.tokensPeakPerMinute}
            label="Token rate"
            unit="tokens/min"
            palette="standby"
            size="fill"
          />
        </div>
        <div class="standby-screen__count" data-role="standby-updated-count">
          <strong class="standby-screen__count-value">
            {numeral(String(model.ticketsUpdated), 'Tickets updated')}
          </strong>
          <span class="standby-screen__count-label">tickets updated · last 60 min</span>
        </div>
      </div>
      {model.alerts.length > 0 && (
        <div class="standby-screen__alerts" role="alert">
          {model.alerts.map((alert) => (
            <div class="standby-screen__alert" data-role="standby-alert" data-word={alert.word} data-key={alert.key}>
              <span class="standby-screen__alert-icon">
                <LucideIcon {...LIVE_STATE_ICONS[alert.word]} inline />
              </span>
              <span class="standby-screen__alert-text">
                <span class="standby-screen__alert-title">{alert.title}</span>
                {alert.detail && <span class="standby-screen__alert-detail">{alert.detail}</span>}
              </span>
            </div>
          ))}
        </div>
      )}
      {model.rows.length > 0 && (
        <ul class="standby-screen__strip" aria-label="Projects">
          {model.rows.map((row) => (
            <li
              class="standby-screen__row"
              data-role="standby-row"
              data-word={row.word}
              data-alert={String(row.alert)}
              data-key={row.key}
            >
              <span class="standby-screen__row-icon">
                <LucideIcon {...LIVE_STATE_ICONS[row.word]} inline />
              </span>
              <span class="standby-screen__row-text">
                <span class="standby-screen__row-title">{row.title}</span>
                {row.detail && <span class="standby-screen__row-detail">{row.detail}</span>}
              </span>
            </li>
          ))}
        </ul>
      )}
      <p class="standby-screen__freshness" data-role="standby-freshness" data-live={String(freshness.live)}>
        <span class="standby-screen__freshness-dot" aria-hidden="true" />
        {freshness.label}
      </p>
    </div>
  );
  // The overlay is a manual popover the top-layer watcher lifts above every app surface, including an
  // open modal dialog; the whole surface is the click target that exits.
  return overlay ? (
    <dialog
      class="standby-screen"
      data-component="standby-screen"
      data-presentation="overlay"
      data-action="exit-standby"
      popover="manual"
      {...{ [TOP_LAYER_OVERLAY_ATTRIBUTE]: '' }}
      tabindex={-1}
      aria-label="Standby"
    >
      {content}
    </dialog>
  ) : (
    <section class="standby-screen" data-component="standby-screen" data-presentation="inline" aria-label="Standby">
      {content}
    </section>
  );
}
