import { CatalogExample } from '@kerfjs/ui/catalog';

import { RegisteredProjectsSettings } from '../components/registered-projects-settings';
import { wireRegisteredProjectsSettings } from '../components/registered-projects-settings-interactions';
import { createRegisteredProjects } from '../features/registered-projects';
import type { RegisteredProject } from '../registered-projects-model';
import type { DemoModule } from './demo-module';

const OPEN_PROJECT = 'web-4c1d2e3f4a5b';

function fixtureList(): RegisteredProject[] {
  return [
    { id: 'api-1a2b3c4d5e6f', alias: 'api', root: '/Users/synthetic/Projects/api', available: true },
    { id: OPEN_PROJECT, alias: 'web', root: '/Users/synthetic/Projects/web', available: true },
    {
      id: 'archive-9f8e7d6c5b4a',
      alias: 'archive',
      root: '/Volumes/Synthetic Backup/old clones/a very long folder name that wraps on a phone/archive',
      available: false,
    },
  ];
}

/**
 * The production controller over an in-memory host: removal edits the fixture, and removing the
 * project marked open is refused the way the server refuses it. No network request and no timer.
 */
function createDemoHost() {
  let list = fixtureList();
  const json = (body: unknown, status = 200) => Response.json(body, { status });
  const request: typeof fetch = (input, init) => {
    const url = new URL(
      typeof input === 'string' ? input : input instanceof URL ? input.href : input.url,
      'http://demo',
    );
    if ((init?.method ?? 'GET') === 'GET') return Promise.resolve(json(list));
    const id = decodeURIComponent(url.pathname.split('/').pop() ?? '');
    if (id === OPEN_PROJECT)
      return Promise.resolve(json({ error: 'web is open in a window; close it before removing it.' }, 409));
    list = list.filter((project) => project.id !== id);
    return Promise.resolve(json({ removed: true }));
  };
  const projects = createRegisteredProjects({ request });
  return {
    projects,
    reset: () => {
      list = fixtureList();
      projects.reset();
      projects.start();
    },
  };
}

const host = createDemoHost();

export function RegisteredProjectsSettingsDemo() {
  return (
    <>
      <CatalogExample label="Interactive">
        <RegisteredProjectsSettings
          state={host.projects.state.value}
          list={host.projects.list.value}
          message={host.projects.message.value}
          busy={host.projects.busy.value}
        />
      </CatalogExample>
      <CatalogExample label="Loading">
        <RegisteredProjectsSettings state="loading" />
      </CatalogExample>
      <CatalogExample label="Empty">
        <RegisteredProjectsSettings state="ready" list={[]} />
      </CatalogExample>
      <CatalogExample label="Removal Refused">
        <RegisteredProjectsSettings
          state="ready"
          list={fixtureList()}
          message="web is open in a window; close it before removing it."
        />
      </CatalogExample>
      <CatalogExample label="Load Failure">
        <RegisteredProjectsSettings
          state="idle"
          message="Could not load the project list. The Hot Sheet server is not responding."
        />
      </CatalogExample>
    </>
  );
}

/** The catalog entry this file renders (HS2-R806FZ): the production controller and handlers over a fixture host. */
export const demoModule: DemoModule = {
  entries: {
    'registered-projects-settings': {
      catalogExamples: true,
      Demo: RegisteredProjectsSettingsDemo,
      reset: () => {
        host.reset();
      },
    },
  },
  wire: (root) => {
    host.projects.start();
    const unwire = wireRegisteredProjectsSettings(root, host.projects);
    return () => {
      unwire();
      host.projects.stop();
    };
  },
};
