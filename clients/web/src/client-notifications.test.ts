import { describe, expect, it, vi } from 'vitest';

import type { Api, ClientNotification } from './api';
import { createClientNotifications } from './client-notifications';
import type { Project } from './interactions/types';

const project = { id: 'checkout-one', name: 'One', apiPath: '/api/one' } as Project;
const notification = (id: string, acknowledged = false): ClientNotification => ({
  id,
  message: `Synthetic notice ${id}`,
  severity: 'info',
  checkout: project.id,
  store: null,
  ticket: null,
  recipient: null,
  dedupe_key: null,
  created_at_ms: 1_000,
  acknowledged,
});

describe('client notification inbox (HS2-ZP869N)', () => {
  it('loads a silent baseline, presents only new live ids once, and acknowledges', async () => {
    let server = [notification('old')];
    const present = vi.fn();
    const api = {
      notifications: vi.fn(async () => server),
      acknowledgeNotification: vi.fn(async (id: string) => notification(id, true)),
    } as unknown as Pick<Api, 'notifications' | 'acknowledgeNotification'>;
    const inbox = createClientNotifications({ api: () => api, present });
    await inbox.refresh(project);
    expect(inbox.pending(project.id)).toHaveLength(1);
    expect(present).not.toHaveBeenCalled();

    server = [notification('old'), notification('new')];
    await inbox.refresh(project, ['new']);
    await inbox.refresh(project, ['new']);
    expect(present).toHaveBeenCalledTimes(1);
    expect(present).toHaveBeenCalledWith(project, notification('new'));

    await inbox.acknowledge(project, 'new');
    expect(inbox.pending(project.id).map((item) => item.id)).toEqual(['old']);
    inbox.removeProject(project.id);
    expect(inbox.items(project.id)).toEqual([]);
  });

  it('keeps a later live refresh after an older load resolves', async () => {
    let finishFirst: ((items: ClientNotification[]) => void) | undefined;
    const first = new Promise<ClientNotification[]>((resolve) => {
      finishFirst = resolve;
    });
    const api = {
      notifications: vi
        .fn()
        .mockReturnValueOnce(first)
        .mockResolvedValueOnce([notification('new')]),
      acknowledgeNotification: vi.fn(),
    } as unknown as Pick<Api, 'notifications' | 'acknowledgeNotification'>;
    const present = vi.fn();
    const inbox = createClientNotifications({ api: () => api, present });
    const baseline = inbox.refresh(project);
    const live = inbox.refresh(project, ['new']);
    finishFirst?.([notification('old')]);
    await Promise.all([baseline, live]);
    expect(inbox.items(project.id).map((item) => item.id)).toEqual(['new']);
    expect(present).toHaveBeenCalledTimes(1);
  });
});
