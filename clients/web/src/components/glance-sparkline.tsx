import './glance-sparkline.css';

/** The series' meaning colour; `neutral` is the dark-cockpit default, problems use `warning`. */
export type GlanceSparklineTone = 'neutral' | 'brand' | 'success' | 'warning';

export interface GlanceSparklineProps {
  /** Samples, oldest first. */
  values: readonly number[];
  /** The hover title and accessible name: what the series counts and over which window. */
  label: string;
  tone?: GlanceSparklineTone;
}

const WIDTH = 120;
const HEIGHT = 28;
const INSET = 2;

/** The polyline points for `values`, scaled to the series' own maximum. */
export function glanceSparklinePoints(values: readonly number[]): string {
  if (values.length === 0) return '';
  const max = Math.max(...values, 0);
  const step = values.length > 1 ? WIDTH / (values.length - 1) : 0;
  return values
    .map((value, index) => {
      const y = max > 0 ? HEIGHT - INSET - (Math.max(0, value) / max) * (HEIGHT - 2 * INSET) : HEIGHT - INSET;
      return `${Math.round(index * step * 10) / 10},${Math.round(y * 10) / 10}`;
    })
    .join(' ');
}

/**
 * A small trend line for a glance surface (HS2-ERTNJ7, designed on HS2-CT5EWR): a line over a soft
 * area, scaled to its own maximum. Kerf has no sparkline, so this is an app-owned domain visualization
 * like the glance gauge. It carries no visible text; the values are in its title and accessible name.
 */
export function GlanceSparkline({ values, label, tone = 'neutral' }: GlanceSparklineProps) {
  const points = glanceSparklinePoints(values);
  const title = `${label}: ${values.join(', ')}`;
  return (
    <svg
      class="glance-sparkline"
      data-component="glance-sparkline"
      data-tone={tone}
      data-empty={String(!values.some((value) => value > 0))}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={title}
    >
      <title>{title}</title>
      {points && <polygon class="glance-sparkline__area" points={`0,${HEIGHT} ${points} ${WIDTH},${HEIGHT}`} />}
      {points && <polyline class="glance-sparkline__line" points={points} />}
    </svg>
  );
}
