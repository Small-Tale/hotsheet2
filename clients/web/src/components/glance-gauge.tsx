import './glance-gauge.css';

import {
  formatGlanceValue,
  GLANCE_GAUGE_CENTER,
  GLANCE_GAUGE_END_ANGLE,
  GLANCE_GAUGE_RADIUS,
  GLANCE_GAUGE_START_ANGLE,
  glanceGaugeAngle,
  glanceGaugeArcPath,
  glanceGaugeFraction,
  glanceGaugePoint,
  glanceGaugeTicks,
  glanceGaugeTitle,
} from './glance-gauge-geometry';

/** `full` draws ticks plus the value and unit in the dial; `bare` is arc, fill, and needle only. */
export type GlanceGaugeVariant = 'full' | 'bare';
/** The fill's meaning colour: brand (blue) for tokens/min, success (green) for tickets updated/h. */
export type GlanceGaugeTone = 'brand' | 'success';
/** `theme` follows the light or dark app theme; `standby` is the dark-red-on-black night palette. */
export type GlanceGaugePalette = 'theme' | 'standby';
/** `chip` (40px) sits at chip height, `medium` (88px), `large` (120px), or `fill` the container width. */
export type GlanceGaugeSize = 'chip' | 'medium' | 'large' | 'fill';

export interface GlanceGaugeProps {
  /** The current reading, in `unit`. */
  value: number;
  /** The series' 24-hour peak; it sets the full-scale end of the dial. */
  peak: number;
  /** What the gauge measures, for the hover title and accessible name, e.g. `Token rate`. */
  label: string;
  /** The reading's unit, e.g. `tokens/min`; drawn in the full dial and named in the title. */
  unit: string;
  variant?: GlanceGaugeVariant;
  tone?: GlanceGaugeTone;
  palette?: GlanceGaugePalette;
  /** Defaults to `chip` for the bare variant and `large` for the full one. */
  size?: GlanceGaugeSize;
}

const NEEDLE_LENGTH = GLANCE_GAUGE_RADIUS - 6;

/**
 * A speedometer for one live rate on a glance surface (HS2-CVE3SK, designed on HS2-CT5EWR): a needle
 * over a 240-degree arc scaled to the series' own 24-hour peak. Kerf has no gauge (KF-Y0XT2R), so this
 * is an app-owned domain visualization. Every text it carries lives in the hover title and accessible
 * name; the full variant also draws the value and unit inside the dial.
 */
export function GlanceGauge({
  value,
  peak,
  label,
  unit,
  variant = 'full',
  tone = 'brand',
  palette = 'theme',
  size,
}: GlanceGaugeProps) {
  const fraction = glanceGaugeFraction(value, peak);
  const angle = glanceGaugeAngle(fraction);
  const title = glanceGaugeTitle(label, value, unit, peak);
  const full = variant === 'full';
  const track = glanceGaugeArcPath(GLANCE_GAUGE_START_ANGLE, GLANCE_GAUGE_END_ANGLE);
  const needleTip = { x: GLANCE_GAUGE_CENTER.x, y: GLANCE_GAUGE_CENTER.y - NEEDLE_LENGTH };
  const percent = Math.round(fraction * 1000) / 10;
  const arcWidth = full ? '7' : '9';
  return (
    <svg
      class="glance-gauge"
      data-component="glance-gauge"
      data-variant={variant}
      data-tone={tone}
      data-palette={palette}
      data-size={size ?? (full ? 'large' : 'chip')}
      data-fraction={String(fraction)}
      data-angle={String(angle)}
      data-empty={String(fraction === 0)}
      viewBox={full ? '0 0 100 86' : '0 0 100 78'}
      role="img"
      aria-label={title}
      style={`--_glance-gauge-angle:${angle}deg;--_glance-gauge-fill:${percent}`}
    >
      <title>{title}</title>
      <path class="glance-gauge__track" d={track} stroke-width={arcWidth} />
      <path class="glance-gauge__fill" d={track} pathLength={100} stroke-width={arcWidth} />
      {full &&
        glanceGaugeTicks().map((tick) => {
          const inner = glanceGaugePoint(tick.angle, GLANCE_GAUGE_RADIUS - 9);
          const outer = glanceGaugePoint(tick.angle, GLANCE_GAUGE_RADIUS - (tick.major ? 3 : 6));
          return (
            <line
              class="glance-gauge__tick"
              data-major={String(tick.major)}
              x1={String(inner.x)}
              y1={String(inner.y)}
              x2={String(outer.x)}
              y2={String(outer.y)}
              stroke-width={tick.major ? '1.4' : '0.8'}
            />
          );
        })}
      <g class="glance-gauge__needle">
        <line
          x1={String(GLANCE_GAUGE_CENTER.x)}
          y1={String(GLANCE_GAUGE_CENTER.y)}
          x2={String(needleTip.x)}
          y2={String(needleTip.y)}
          stroke-width={full ? '2.4' : '4'}
        />
        <circle cx={String(GLANCE_GAUGE_CENTER.x)} cy={String(GLANCE_GAUGE_CENTER.y)} r={full ? '4' : '6'} />
      </g>
      {full && (
        <text class="glance-gauge__value" x="50" y="73" text-anchor="middle" font-size="14">
          {formatGlanceValue(value)}
        </text>
      )}
      {full && (
        <text class="glance-gauge__unit" x="50" y="83" text-anchor="middle" font-size="8">
          {unit}
        </text>
      )}
    </svg>
  );
}
