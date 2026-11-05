/**
 * Pure scale, angle, and label math for the glance gauge (HS2-CVE3SK). The dial sweeps 240 degrees,
 * from -120 (lower left) through 0 (straight up) to +120 (lower right), and is scaled to the series'
 * own 30-day peak, as designed on HS2-CT5EWR.
 */

/** Needle angle at an empty dial, in degrees clockwise from straight up. */
export const GLANCE_GAUGE_START_ANGLE = -120;
/** Needle angle at the 30-day peak. */
export const GLANCE_GAUGE_END_ANGLE = 120;
/** Dial centre and arc radius, in viewBox units (the viewBox is 100 wide). */
export const GLANCE_GAUGE_CENTER = { x: 50, y: 52 } as const;
export const GLANCE_GAUGE_RADIUS = 40;

/**
 * The share of the dial a reading fills: `value / peak`, pinned to the dial's ends. A reading above
 * its peak pins to the end rather than wrapping, and a non-finite or negative reading reads as empty.
 * Without a positive peak any positive reading is its own peak.
 */
export function glanceGaugeFraction(value: number, peak: number): number {
  if (!Number.isFinite(value) || value <= 0) return 0;
  if (!Number.isFinite(peak) || peak <= 0) return 1;
  return Math.min(1, value / peak);
}

/** Needle angle, in degrees clockwise from straight up, for a dial fraction. */
export function glanceGaugeAngle(fraction: number): number {
  const pinned = Math.min(1, Math.max(0, fraction));
  return GLANCE_GAUGE_START_ANGLE + (GLANCE_GAUGE_END_ANGLE - GLANCE_GAUGE_START_ANGLE) * pinned;
}

/** A point on a circle around the dial centre, at an angle clockwise from straight up. */
export function glanceGaugePoint(angle: number, radius: number): { x: number; y: number } {
  const radians = ((angle - 90) * Math.PI) / 180;
  return {
    x: round(GLANCE_GAUGE_CENTER.x + radius * Math.cos(radians)),
    y: round(GLANCE_GAUGE_CENTER.y + radius * Math.sin(radians)),
  };
}

/** SVG path data for a clockwise arc around the dial centre. */
export function glanceGaugeArcPath(from: number, to: number, radius: number = GLANCE_GAUGE_RADIUS): string {
  const start = glanceGaugePoint(from, radius);
  const end = glanceGaugePoint(to, radius);
  const largeArc = to - from > 180 ? 1 : 0;
  return `M${start.x} ${start.y} A${radius} ${radius} 0 ${largeArc} 1 ${end.x} ${end.y}`;
}

/** Tick angles for the full dial: eleven ticks, every tenth of the scale, majors at 0, 50, and 100%. */
export function glanceGaugeTicks(): { angle: number; major: boolean }[] {
  return Array.from({ length: 11 }, (_, index) => ({
    angle: glanceGaugeAngle(index / 10),
    major: index % 5 === 0,
  }));
}

const COMPACT_NUMBER = new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 });

/** A reading as a short raw number for a compact surface: `14`, `2.5`, `1.6k`, `3.2m`. */
export function formatGlanceValue(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0';
  return COMPACT_NUMBER.format(value).toLowerCase();
}

/**
 * The gauge's hover title and accessible name: what it measures, the reading with its unit, and the
 * 30-day peak that sets its scale (HS2-CT5EWR: "just show text via title / on hover").
 */
export function glanceGaugeTitle(label: string, value: number, unit: string, peak: number): string {
  const reading = `${label}: ${formatGlanceValue(value)} ${unit}`;
  if (!Number.isFinite(peak) || peak <= 0) return `${reading} (no 30-day peak yet)`;
  return `${reading} (30-day peak ${formatGlanceValue(peak)})`;
}

function round(value: number): number {
  return Math.round(value * 100) / 100;
}
