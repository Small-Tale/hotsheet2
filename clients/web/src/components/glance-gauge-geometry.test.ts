import { describe, expect, it } from 'vitest';

import {
  formatGlanceValue,
  GLANCE_GAUGE_END_ANGLE,
  GLANCE_GAUGE_START_ANGLE,
  glanceGaugeAngle,
  glanceGaugeArcPath,
  glanceGaugeFraction,
  glanceGaugePoint,
  glanceGaugeTicks,
  glanceGaugeTitle,
} from './glance-gauge-geometry';

describe('glance gauge scale (HS2-CVE3SK)', () => {
  it('scales a reading to its own 24-hour peak', () => {
    expect(glanceGaugeFraction(0, 6200)).toBe(0);
    expect(glanceGaugeFraction(1550, 6200)).toBe(0.25);
    expect(glanceGaugeFraction(6200, 6200)).toBe(1);
  });

  it('pins a reading above its peak to the end of the dial', () => {
    expect(glanceGaugeFraction(9000, 6200)).toBe(1);
  });

  it('reads negative and non-finite values as empty', () => {
    expect(glanceGaugeFraction(-5, 10)).toBe(0);
    expect(glanceGaugeFraction(Number.NaN, 10)).toBe(0);
    expect(glanceGaugeFraction(Number.POSITIVE_INFINITY, 10)).toBe(0);
  });

  it('treats a positive reading without a positive peak as its own peak', () => {
    expect(glanceGaugeFraction(3, 0)).toBe(1);
    expect(glanceGaugeFraction(3, Number.NaN)).toBe(1);
    expect(glanceGaugeFraction(0, 0)).toBe(0);
  });
});

describe('glance gauge angles (HS2-CVE3SK)', () => {
  it('sweeps 240 degrees from lower left through straight up to lower right', () => {
    expect(glanceGaugeAngle(0)).toBe(GLANCE_GAUGE_START_ANGLE);
    expect(glanceGaugeAngle(0.5)).toBe(0);
    expect(glanceGaugeAngle(1)).toBe(GLANCE_GAUGE_END_ANGLE);
    expect(glanceGaugeAngle(0.25)).toBe(-60);
  });

  it('pins out-of-range fractions to the dial ends', () => {
    expect(glanceGaugeAngle(-1)).toBe(GLANCE_GAUGE_START_ANGLE);
    expect(glanceGaugeAngle(2)).toBe(GLANCE_GAUGE_END_ANGLE);
  });

  it('places points clockwise from straight up around the centre', () => {
    expect(glanceGaugePoint(0, 40)).toEqual({ x: 50, y: 12 });
    expect(glanceGaugePoint(90, 40)).toEqual({ x: 90, y: 52 });
    expect(glanceGaugePoint(-120, 40)).toEqual({ x: 15.36, y: 72 });
    expect(glanceGaugePoint(120, 40)).toEqual({ x: 84.64, y: 72 });
  });

  it('draws the 240-degree track as one large clockwise arc', () => {
    expect(glanceGaugeArcPath(-120, 120)).toBe('M15.36 72 A40 40 0 1 1 84.64 72');
    expect(glanceGaugeArcPath(-120, 0)).toBe('M15.36 72 A40 40 0 0 1 50 12');
  });

  it('places eleven ticks with majors at empty, half, and peak', () => {
    const ticks = glanceGaugeTicks();
    expect(ticks).toHaveLength(11);
    expect(ticks.filter((tick) => tick.major).map((tick) => tick.angle)).toEqual([-120, 0, 120]);
    expect(ticks[1]?.angle).toBe(-96);
  });
});

describe('glance gauge text (HS2-CVE3SK)', () => {
  it('formats readings as short raw numbers', () => {
    expect(formatGlanceValue(0)).toBe('0');
    expect(formatGlanceValue(14)).toBe('14');
    expect(formatGlanceValue(2.5)).toBe('2.5');
    expect(formatGlanceValue(1612)).toBe('1.6k');
    expect(formatGlanceValue(6200)).toBe('6.2k');
    expect(formatGlanceValue(3_240_000)).toBe('3.2m');
    expect(formatGlanceValue(-3)).toBe('0');
    expect(formatGlanceValue(Number.NaN)).toBe('0');
  });

  it('names the label, reading, unit, and 24-hour peak', () => {
    expect(glanceGaugeTitle('Token rate', 1600, 'tokens/min', 6200)).toBe(
      'Token rate: 1.6k tokens/min (24-hour peak 6.2k)',
    );
    expect(glanceGaugeTitle('Tickets updated', 14, 'tickets/h', 22)).toBe(
      'Tickets updated: 14 tickets/h (24-hour peak 22)',
    );
  });

  it('says when there is no 24-hour peak yet', () => {
    expect(glanceGaugeTitle('Token rate', 0, 'tokens/min', 0)).toBe('Token rate: 0 tokens/min (no 24-hour peak yet)');
  });

  it('names a shared scale when one is given (HS2-VFQM4D)', () => {
    expect(glanceGaugeTitle('Token rate', 1600, 'tokens/min', 6200, 'All projects 24-hour peak')).toBe(
      'Token rate: 1.6k tokens/min (All projects 24-hour peak 6.2k)',
    );
    expect(glanceGaugeTitle('Token rate', 0, 'tokens/min', 0, 'All projects 24-hour peak')).toBe(
      'Token rate: 0 tokens/min (no All projects 24-hour peak yet)',
    );
  });
});
