import { describe, expect, it } from 'vitest';

import type { RegisteredProject } from '../registered-projects-model';
import { RegisteredProjectsSettings } from './registered-projects-settings';

const projects: RegisteredProject[] = [
  { id: 'api-1', alias: 'api', root: '/synthetic/api', available: true },
  { id: 'old-2', alias: '', root: '/Volumes/Backup/old-clone', available: false },
];
const render = (props: Parameters<typeof RegisteredProjectsSettings>[0]) => String(RegisteredProjectsSettings(props));

describe('HS2-R806FZ App Settings → Projects', () => {
  it('lists each project with its folder and a Remove from Hot Sheet row command', () => {
    const markup = render({ state: 'ready', list: projects });
    expect(markup).toContain('Removing one never deletes its files or ticket store.');
    expect(markup).toContain('data-registered-project="api-1"');
    expect(markup).toContain('/synthetic/api');
    expect(markup).toContain('data-action="remove-registered-project"');
    expect(markup).toContain('data-project-id="old-2"');
    expect(markup).toContain('Remove from Hot Sheet');
    expect(markup).toContain('Actions for api');
  });

  it('marks a missing folder and names an alias-less project after its folder', () => {
    const markup = render({ state: 'ready', list: projects });
    expect(markup.match(/Folder Missing/g)).toHaveLength(1);
    expect(markup).toContain('data-project-available="false"');
    expect(markup).toContain('data-project-available="true"');
    expect(markup).toContain('folder-x');
    expect(markup).toContain('Actions for old-clone');
  });

  it('distinguishes loading, empty, idle, busy, and failed states', () => {
    const loading = render({ state: 'loading' });
    expect(loading).toContain('data-placeholder');
    expect(loading).not.toContain('No projects are registered.');

    expect(render({ state: 'ready', list: [] })).toContain('No projects are registered.');

    const failed = render({ state: 'idle', message: 'Could not load the project list. Down.' });
    expect(failed).toContain('role="alert"');
    expect(failed).not.toContain('data-placeholder');
    expect(failed).not.toContain('No projects are registered.');

    expect(render({ state: 'ready', list: projects, busy: ['api-1'] })).toContain('aria-busy="true"');
  });
});
