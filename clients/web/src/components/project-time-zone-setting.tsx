import { Select } from '@kerfjs/ui/select';

import { PROJECT_TIME_ZONE_FIELD, projectTimeZoneChoices } from '../project-time-zone';

export interface ProjectTimeZoneSettingProps {
  /** The project's zone in effect; absent while it is read for the first time. */
  timeZone?: string;
  /** Read in progress; the control is a placeholder only until the zone is first known. */
  loading?: boolean;
  /** A refused save or a failed read, shown under the control where it can be corrected. */
  message?: string;
  /** Names a hand-edited shared value that is not a zone; the project reads as UTC meanwhile. */
  diagnostic?: string;
  /** The instant the offsets in the labels describe; the catalog pins it, the app uses now. */
  at?: Date;
  /** The zone list; the catalog pins it, the app uses every zone the runtime knows. */
  zones?: readonly string[];
}

/**
 * Project Settings → General → Time Zone (HS2-SZF152, docs/38-milestones.md §38.3): the project's shared
 * `time_zone`, the zone every due date ends in for everyone on the project. A choice saves at once; the
 * headless twin is `hotsheet-cli settings set time_zone <zone>`.
 */
export function ProjectTimeZoneSetting({
  timeZone,
  loading = false,
  message = '',
  diagnostic = '',
  at,
  zones,
}: ProjectTimeZoneSettingProps) {
  const pending = loading && timeZone === undefined,
    { value, choices } = projectTimeZoneChoices(timeZone, at, zones);
  return (
    <section data-component="project-time-zone-setting" data-time-zone={timeZone ?? ''}>
      <Select
        name={PROJECT_TIME_ZONE_FIELD}
        label="Time Zone"
        value={value}
        choices={choices}
        placeholder={pending}
        disabled={pending}
        hint="Due dates end at midnight in this time zone for everyone on this project."
        error={message || diagnostic || undefined}
      />
    </section>
  );
}
