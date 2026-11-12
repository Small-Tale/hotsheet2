import { describe, expect, it } from 'vitest';

import { CPU_CORE_METER_SEGMENTS, CpuCoreMeter, cpuCoreMeterLevel } from './cpu-core-meter';

const lit = (html: string) => (html.match(/data-lit="true"/gu) ?? []).length;

describe('HS2-CPFBWP CpuCoreMeter', () => {
  it('lights one segment per started fifth, none when idle', () => {
    expect([0, 0.1, 20, 20.1, 59, 80, 80.1, 100, 140, Number.NaN].map(cpuCoreMeterLevel)).toEqual([
      0, 1, 1, 2, 3, 4, 5, 5, 5, 0,
    ]);
  });

  it('draws one column per core with the peak segment marked, values in its name', () => {
    const html = String(CpuCoreMeter({ cores: [100, 0, 41] }));
    expect(html).toContain('data-cores="3"');
    expect((html.match(/<rect/gu) ?? []).length).toBe(3 * CPU_CORE_METER_SEGMENTS);
    expect(lit(html)).toBe(5 + 0 + 3);
    expect(html).toContain('aria-label="3 cores: core 1 100%, core 2 0%, core 3 41%"');
    expect(html).toContain('--_cpu-core-meter-width: 1.375rem');
    // An empty list still renders a valid, empty meter.
    expect(String(CpuCoreMeter({ cores: [] }))).toContain('data-cores="0"');
  });
});
