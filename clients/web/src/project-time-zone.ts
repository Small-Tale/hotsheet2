import { supportedTimeZones } from './notification-settings-model';

/** The zone a project uses when it sets none (docs/38-milestones.md §38.3, HS2-SZF152); never the device's. */
export const DEFAULT_PROJECT_TIME_ZONE = 'UTC';

/** The Time Zone Select's form name, which the application and catalog wiring delegate on. */
export const PROJECT_TIME_ZONE_FIELD = 'project-time-zone';

export interface ProjectTimeZoneChoices {
  value: string;
  choices: { value: string; label: string }[];
}

/** The last choice list, reused while its saved zone, hour, and zone list are unchanged. */
let lastChoices: { key: string; result: ProjectTimeZoneChoices } | undefined;
const offsetFormatters = new Map<string, Intl.DateTimeFormat | undefined>();

function offsetFormatter(zone: string): Intl.DateTimeFormat | undefined {
  if (!offsetFormatters.has(zone)) {
    let formatter: Intl.DateTimeFormat | undefined;
    try {
      formatter = new Intl.DateTimeFormat('en-US', { timeZone: zone, timeZoneName: 'longOffset' });
    } catch {
      formatter = undefined;
    }
    offsetFormatters.set(zone, formatter);
  }
  return offsetFormatters.get(zone);
}

/**
 * A zone's offset from UTC at `at`, as `UTC+01:00` (`UTC+00:00` for no offset), or nothing when this
 * runtime does not know the zone. The offset follows daylight saving, so it is the offset in effect now.
 */
export function zoneOffsetLabel(zone: string, at: Date): string | undefined {
  const part = offsetFormatter(zone)
    ?.formatToParts(at)
    .find((entry) => entry.type === 'timeZoneName')?.value;
  if (part === undefined) return undefined;
  const offset = part.replace(/^GMT/, '');
  return `UTC${offset || '+00:00'}`;
}

/** A zone's choice label: its name, readable, with its current offset (`Europe/Berlin (UTC+01:00)`). */
export function projectTimeZoneLabel(zone: string, at: Date): string {
  if (zone === DEFAULT_PROJECT_TIME_ZONE) return DEFAULT_PROJECT_TIME_ZONE;
  const offset = zoneOffsetLabel(zone, at);
  const name = zone.replaceAll('_', ' ');
  return offset ? `${name} (${offset})` : name;
}

/**
 * The Project Settings › General › Time Zone choices (HS2-SZF152): `UTC` first, then every zone this runtime
 * knows, each labeled with its current offset, the same zone list as the notifier quiet-hours control. The
 * saved zone stays selected even when this runtime does not list it; an unset project reads as `UTC`.
 */
export function projectTimeZoneChoices(
  saved: string | undefined,
  at: Date = new Date(),
  zones: readonly string[] = supportedTimeZones(),
): ProjectTimeZoneChoices {
  const value = saved || DEFAULT_PROJECT_TIME_ZONE;
  // Offsets change at most on the hour, so an unchanged list is reused within the hour.
  const key = `${value}\n${String(Math.floor(at.getTime() / 3_600_000))}\n${zones.join('\n')}`;
  if (lastChoices?.key === key) return lastChoices.result;
  const names = new Set([DEFAULT_PROJECT_TIME_ZONE]);
  const others = [...zones, value].filter((zone) => {
    if (names.has(zone)) return false;
    names.add(zone);
    return true;
  });
  others.sort((first, second) => first.localeCompare(second, 'en'));
  const result = {
    value,
    choices: [DEFAULT_PROJECT_TIME_ZONE, ...others].map((zone) => ({
      value: zone,
      label: projectTimeZoneLabel(zone, at),
    })),
  };
  lastChoices = { key, result };
  return result;
}
