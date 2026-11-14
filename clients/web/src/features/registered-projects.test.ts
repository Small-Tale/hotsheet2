import { describe, expect, it } from 'vitest';

import { REGISTERED_PROJECTS_PATH, type RegisteredProject } from '../registered-projects-model';
import { createRegisteredProjects } from './registered-projects';

const sample = (): RegisteredProject[] => [
  { id: 'web-2', alias: 'web', root: '/synthetic/web', available: true },
  { id: 'api-1', alias: 'API', root: '/synthetic/api', available: false },
];

/** An in-memory bridge that records every request. `refuse` names an id whose removal 409s. */
function fakeHost(options: { refuse?: string; failList?: number } = {}) {
  let list = sample();
  let failures = options.failList ?? 0;
  const calls: string[] = [];
  let release: (() => void) | undefined;
  const request: typeof fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const method = init?.method ?? 'GET';
    calls.push(`${method} ${url}`);
    if (method === 'GET') {
      if (failures > 0) {
        failures -= 1;
        return Response.json({ error: 'server down' }, { status: 502 });
      }
      return Response.json(list);
    }
    if (release === undefined && url.endsWith('/hold')) await new Promise<void>((r) => (release = r));
    const id = decodeURIComponent(url.split('/').pop() ?? '');
    if (id === options.refuse) return Response.json({ error: `${id} is open in a window` }, { status: 409 });
    list = list.filter((project) => project.id !== id);
    return Response.json({ removed: true });
  };
  return { request, calls, unblock: () => release?.() };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('HS2-R806FZ registered projects controller', () => {
  it('loads sorted on start, keeps the list across stop, and reloads on the next start', async () => {
    const host = fakeHost();
    const projects = createRegisteredProjects({ request: host.request });
    expect(projects.state.value).toBe('idle');
    projects.start();
    expect(projects.state.value).toBe('loading');
    await settle();
    expect(projects.state.value).toBe('ready');
    expect(projects.list.value?.map((p) => p.id)).toEqual(['api-1', 'web-2']);
    projects.stop();
    expect(projects.state.value).toBe('ready');
    projects.start();
    expect(projects.state.value).toBe('ready');
    await settle();
    expect(host.calls).toEqual([`GET ${REGISTERED_PROJECTS_PATH}`, `GET ${REGISTERED_PROJECTS_PATH}`]);
  });

  it('removes a project and reloads; a refusal shows the reason and keeps the list', async () => {
    const host = fakeHost({ refuse: 'web-2' });
    const projects = createRegisteredProjects({ request: host.request });
    projects.start();
    await settle();
    expect(await projects.remove('web-2')).toBe(false);
    expect(projects.message.value).toBe('web-2 is open in a window');
    expect(projects.list.value).toHaveLength(2);
    expect(await projects.remove('api-1')).toBe(true);
    expect(projects.message.value).toBe('');
    expect(projects.list.value?.map((p) => p.id)).toEqual(['web-2']);
    expect(projects.busy.value).toEqual([]);
    expect(host.calls).toContain(`DELETE ${REGISTERED_PROJECTS_PATH}/api-1`);
  });

  it('reports a load failure, then clears it on a later successful load', async () => {
    const host = fakeHost({ failList: 1 });
    const projects = createRegisteredProjects({ request: host.request });
    projects.start();
    await settle();
    expect(projects.state.value).toBe('idle');
    expect(projects.message.value).toBe('Could not load the project list. server down');
    projects.start();
    await settle();
    expect(projects.state.value).toBe('ready');
    expect(projects.message.value).toBe('');
  });

  it('ignores a load that finishes after the category closed, and a repeated removal while busy', async () => {
    const host = fakeHost();
    const projects = createRegisteredProjects({ request: host.request });
    projects.start();
    projects.stop();
    await settle();
    expect(projects.list.value).toBeUndefined();
    expect(projects.state.value).toBe('idle');

    const first = projects.remove('hold');
    expect(await projects.remove('hold')).toBe(false);
    host.unblock();
    expect(await first).toBe(true);

    projects.reset();
    expect(projects.list.value).toBeUndefined();
    expect(projects.message.value).toBe('');
  });
});
