import './cpu-core-meter.css';

export interface CpuCoreMeterProps {
  /** Each logical core's busy percent, in OS order. */
  cores: readonly number[];
}

/** Lit segments per column; the top segment lights only at 80% or more. */
export const CPU_CORE_METER_SEGMENTS = 5;
const COLUMN = 6;
const GAP = 2;
const SEGMENT = 3;
const SEGMENT_GAP = 1;
const HEIGHT = CPU_CORE_METER_SEGMENTS * SEGMENT + (CPU_CORE_METER_SEGMENTS - 1) * SEGMENT_GAP;

/** How many segments a core's busy percent lights: one per started fifth, none when idle. */
export function cpuCoreMeterLevel(percent: number): number {
  if (!(percent > 0)) return 0;
  return Math.min(CPU_CORE_METER_SEGMENTS, Math.ceil(percent / (100 / CPU_CORE_METER_SEGMENTS)));
}

/**
 * A per-core busy meter for the This Mac CPU card (HS2-CPFBWP): one LED column per logical core, lit
 * from the bottom like a VU meter, with the top segment in the warning tone. Kerf has no meter for
 * this, so it is an app-owned domain visualization like the glance sparkline. The values are in its
 * title and accessible name; it carries no visible text.
 */
export function CpuCoreMeter({ cores }: CpuCoreMeterProps) {
  const width = Math.max(1, cores.length * (COLUMN + GAP) - GAP);
  const label = `${cores.length} cores: ${cores.map((value, index) => `core ${index + 1} ${Math.round(value)}%`).join(', ')}`;
  return (
    <svg
      class="cpu-core-meter"
      data-component="cpu-core-meter"
      data-cores={String(cores.length)}
      viewBox={`0 0 ${width} ${HEIGHT}`}
      preserveAspectRatio="none"
      style={`--_cpu-core-meter-width: ${width / 16}rem`}
      role="img"
      aria-label={label}
    >
      <title>{label}</title>
      {cores.map((value, column) => {
        const lit = cpuCoreMeterLevel(value);
        return Array.from({ length: CPU_CORE_METER_SEGMENTS }, (_, segment) => (
          <rect
            class="cpu-core-meter__segment"
            data-lit={String(segment < lit)}
            data-peak={String(segment === CPU_CORE_METER_SEGMENTS - 1)}
            x={column * (COLUMN + GAP)}
            y={HEIGHT - (segment + 1) * SEGMENT - segment * SEGMENT_GAP}
            width={COLUMN}
            height={SEGMENT}
            rx={0.5}
          />
        ));
      })}
    </svg>
  );
}
