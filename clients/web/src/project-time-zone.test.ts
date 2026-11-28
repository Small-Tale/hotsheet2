import { describe, expect, it } from 'vitest';

import {
  DEFAULT_PROJECT_TIME_ZONE,
  projectTimeZoneChoices,
  projectTimeZoneLabel,
  zoneOffsetLabel,
} from './project-time-zone';

const winter = new Date('2026-12-04T12:00:00Z'),
  summer = new Date('2026-07-04T12:00:00Z');

describe('project time zone choices (HS2-SZF152)', () => {
  it('labels each zone with its name and the offset in effect, following daylight saving', () => {
    expect(zoneOffsetLabel('Europe/Berlin', winter)).toBe('UTC+01:00');
    expect(zoneOffsetLabel('Europe/Berlin', summer)).toBe('UTC+02:00');
    expect(zoneOffsetLabel('America/New_York', winter)).toBe('UTC-05:00');
    expect(zoneOffsetLabel('Asia/Kolkata', winter)).toBe('UTC+05:30');
    expect(zoneOffsetLabel('Africa/Abidjan', winter)).toBe('UTC+00:00');
    expect(zoneOffsetLabel('Mars/Olympus_Mons', winter)).toBeUndefined();
    expect(projectTimeZoneLabel('America/Los_Angeles', summer)).toBe('America/Los Angeles (UTC-07:00)');
    expect(projectTimeZoneLabel('UTC', summer)).toBe('UTC');
    // A zone this runtime cannot name keeps its readable name.
    expect(projectTimeZoneLabel('Mars/Olympus_Mons', summer)).toBe('Mars/Olympus Mons');
  });

  it('defaults an unset project to UTC, never the device zone, and lists UTC first', () => {
    for (const saved of [undefined, '']) {
      const { value, choices } = projectTimeZoneChoices(saved, winter, ['Europe/Berlin', 'Asia/Tokyo']);
      expect(value).toBe(DEFAULT_PROJECT_TIME_ZONE);
      expect(choices.map((choice) => choice.value)).toEqual(['UTC', 'Asia/Tokyo', 'Europe/Berlin']);
    }
  });

  it('keeps a saved zone the runtime does not list, once, in name order', () => {
    const { value, choices } = projectTimeZoneChoices('Europe/Kyiv', winter, ['Asia/Tokyo', 'UTC', 'Europe/Berlin']);
    expect(value).toBe('Europe/Kyiv');
    expect(choices.map((choice) => choice.value)).toEqual(['UTC', 'Asia/Tokyo', 'Europe/Berlin', 'Europe/Kyiv']);
    // Listed zones are not repeated when saved.
    const again = projectTimeZoneChoices('Asia/Tokyo', winter, ['Asia/Tokyo', 'Asia/Tokyo']);
    expect(again.choices.map((choice) => choice.value)).toEqual(['UTC', 'Asia/Tokyo']);
  });

  it('offers every zone the runtime knows by default', () => {
    const { choices } = projectTimeZoneChoices(undefined, winter);
    expect(choices.length).toBeGreaterThan(300);
    expect(choices).toContainEqual({ value: 'Europe/Berlin', label: 'Europe/Berlin (UTC+01:00)' });
  });
});
