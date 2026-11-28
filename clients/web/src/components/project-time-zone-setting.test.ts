import { describe, expect, it } from 'vitest';

import { ProjectTimeZoneSetting } from './project-time-zone-setting';

const at = new Date('2026-12-04T12:00:00Z'),
  zones = ['Europe/Berlin', 'America/New_York'];

describe('ProjectTimeZoneSetting (HS2-SZF152)', () => {
  it('renders a Time Zone select of UTC and every zone with its offset, on the saved zone', () => {
    const markup = String(ProjectTimeZoneSetting({ timeZone: 'Europe/Berlin', at, zones }));
    expect(markup).toContain('data-component="project-time-zone-setting"');
    expect(markup).toContain('data-time-zone="Europe/Berlin"');
    expect(markup).toContain('name="project-time-zone"');
    expect(markup).toContain('Time Zone');
    expect(markup).toContain('value="Europe/Berlin"');
    expect(markup).toContain('Europe/Berlin (UTC+01:00)');
    expect(markup).toContain('America/New York (UTC-05:00)');
    expect(markup).toContain('Due dates end at midnight in this time zone');
    expect(markup).not.toContain('aria-invalid="true"');
    expect(markup).not.toMatch(/<kui-select[^>]*disabled/);
  });

  it('reads an unset project as UTC and shows a loading placeholder only before the zone is known', () => {
    const unset = String(ProjectTimeZoneSetting({ at, zones }));
    expect(unset).toContain('value="UTC"');
    const loading = String(ProjectTimeZoneSetting({ loading: true, at, zones }));
    expect(loading).toContain('data-placeholder="true"');
    expect(loading).toContain('aria-busy="true"');
    // Once the zone is known, a later quiet refresh keeps the live control.
    const known = String(ProjectTimeZoneSetting({ timeZone: 'UTC', loading: true, at, zones }));
    expect(known).not.toContain('data-placeholder="true"');
    expect(known).not.toMatch(/<kui-select[^>]*disabled/);
  });

  it('shows a refusal or a hand-edited bad value as the control error', () => {
    const refused = String(
      ProjectTimeZoneSetting({ timeZone: 'UTC', at, zones, message: '"Etc/Unknown" is not a known time zone.' }),
    );
    expect(refused).toContain('aria-invalid="true"');
    expect(refused).toContain('is not a known time zone.');
    const diagnosed = String(
      ProjectTimeZoneSetting({ timeZone: 'UTC', at, zones, diagnostic: 'The project time zone "Mars" is not known.' }),
    );
    expect(diagnosed).toContain('aria-invalid="true"');
    expect(diagnosed).toContain('The project time zone');
  });
});
