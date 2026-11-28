import { CatalogExample } from '@kerfjs/ui/catalog';

import type { TimeZoneSettings } from '../api';
import { ProjectTimeZoneSetting } from '../components/project-time-zone-setting';
import { createProjectTimeZoneSettings, wireProjectTimeZoneSetting } from '../features/project-time-zone-settings';
import type { DemoModule } from './demo-module';

const DEMO_PROJECT = 'demo-project',
  // The fixed states pin the instant and zone list so their offsets do not change with the date.
  WINTER = new Date('2026-12-04T12:00:00Z'),
  ZONES = ['America/New_York', 'Asia/Kolkata', 'Europe/Berlin', 'Europe/London'];

/** An in-memory project whose server refuses a zone named `Etc/Unknown`, as the real route would. */
function demoClient() {
  let stored = 'UTC';
  return {
    timeZoneSettings: (): Promise<TimeZoneSettings> => Promise.resolve({ time_zone: stored }),
    saveTimeZoneSettings: (_projectId: string, zone: string): Promise<TimeZoneSettings> => {
      if (zone === 'Etc/Unknown')
        return Promise.reject(
          new Error(`"${zone}" is not a known time zone. Use a zone name such as Europe/Berlin, or UTC.`),
        );
      stored = zone;
      return Promise.resolve({ time_zone: stored });
    },
  };
}

let client = demoClient();
const demo = createProjectTimeZoneSettings({ client: () => client });

/** Project Settings → General → Time Zone (HS2-SZF152): the production controller over an in-memory project. */
export function ProjectTimeZoneSettingDemo() {
  return (
    <>
      <CatalogExample label="Interactive">
        <ProjectTimeZoneSetting {...demo.view(DEMO_PROJECT)} />
      </CatalogExample>
      <CatalogExample label="Unset (UTC)">
        <ProjectTimeZoneSetting timeZone="UTC" at={WINTER} zones={ZONES} />
      </CatalogExample>
      <CatalogExample label="Chosen Zone">
        <ProjectTimeZoneSetting timeZone="Europe/Berlin" at={WINTER} zones={ZONES} />
      </CatalogExample>
      <CatalogExample label="Loading">
        <ProjectTimeZoneSetting loading at={WINTER} zones={ZONES} />
      </CatalogExample>
      <CatalogExample label="Refused Zone">
        <ProjectTimeZoneSetting
          timeZone="Europe/Berlin"
          at={WINTER}
          zones={ZONES}
          message={'"Etc/Unknown" is not a known time zone. Use a zone name such as Europe/Berlin, or UTC.'}
        />
      </CatalogExample>
      <CatalogExample label="Hand-Edited Unknown Zone">
        <ProjectTimeZoneSetting
          timeZone="UTC"
          at={WINTER}
          zones={ZONES}
          diagnostic={
            'The project time zone "Mars/Olympus_Mons" in .hotsheet2/settings.json is not a known time zone, so due dates use UTC. Choose a zone such as Europe/Berlin, or UTC.'
          }
        />
      </CatalogExample>
    </>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'project-time-zone-setting': {
      catalogExamples: true,
      Demo: ProjectTimeZoneSettingDemo,
      reset: () => {
        client = demoClient();
        demo.reset();
        void demo.refresh(DEMO_PROJECT);
      },
    },
  },
  wire: (root) => {
    client = demoClient();
    demo.reset();
    void demo.refresh(DEMO_PROJECT);
    return wireProjectTimeZoneSetting(root, demo, () => DEMO_PROJECT);
  },
};
