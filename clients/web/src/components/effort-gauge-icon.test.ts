import { Gauge } from 'lucide';
import { describe, expect, it } from 'vitest';

import { effortGaugeIcon } from './effort-gauge-icon';

describe('effortGaugeIcon', () => {
  it('moves the needle progressively from left to right while retaining the Lucide arc', () => {
    expect(['low', 'medium', 'high', 'xhigh', 'max'].map((level) => effortGaugeIcon(level)[0][1].d)).toEqual([
      'M12 14 8 10',
      'M12 14 10 9',
      'M12 14 12 8',
      'M12 14 14 9',
      'm12 14 4-4',
    ]);
    for (const level of ['low', 'medium', 'high', 'xhigh', 'max']) expect(effortGaugeIcon(level)[1]).toBe(Gauge[1]);
  });

  it('uses the stock Lucide gauge for unknown plugin-defined levels', () => {
    expect(effortGaugeIcon('ultra')).toBe(Gauge);
    expect(effortGaugeIcon(undefined)).toBe(Gauge);
  });
});
