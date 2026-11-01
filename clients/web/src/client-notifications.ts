import { signal } from 'kerfjs';

import type { Api, ClientNotification } from './api';
import type { Project } from './interactions/types';

type NotificationApi = Pick<Api, 'notifications' | 'acknowledgeNotification'>;

/** Server-backed attention inbox, keyed by checkout and refreshed only on live events (HS2-ZP869N). */
export function createClientNotifications(options: {
  api: (project: Project) => NotificationApi;
  present: (project: Project, notification: ClientNotification) => void;
}) {
  const byProject = signal<Record<string, ClientNotification[]>>({});
  const presented = new Set<string>();
  const requests = new Map<string, Promise<void>>();
  const generations = new Map<string, number>();

  const items = (projectId: string) => byProject.value[projectId] ?? [];
  const pending = (projectId: string) => items(projectId).filter((item) => !item.acknowledged);

  function refresh(project: Project, announcedIds: readonly string[] = []): Promise<void> {
    const previous = requests.get(project.id);
    const generation = (generations.get(project.id) ?? 0) + 1;
    generations.set(project.id, generation);
    const request = (async () => {
      // An event that follows the initial snapshot must observe that snapshot first.
      if (previous) await previous.catch(() => undefined);
      const next = await options.api(project).notifications(project.id);
      if (generations.get(project.id) !== generation) return;
      byProject.value = { ...byProject.value, [project.id]: next };
      const announced = new Set(announcedIds);
      for (const item of next) {
        if (!announced.has(item.id) || item.acknowledged || presented.has(item.id)) continue;
        presented.add(item.id);
        options.present(project, item);
      }
    })();
    requests.set(project.id, request);
    const settled = () => {
      if (requests.get(project.id) === request) requests.delete(project.id);
    };
    void request.then(settled, settled);
    return request;
  }

  async function acknowledge(project: Project, id: string): Promise<void> {
    const updated = await options.api(project).acknowledgeNotification(id);
    byProject.value = {
      ...byProject.value,
      [project.id]: items(project.id).map((item) => (item.id === id ? updated : item)),
    };
  }

  function removeProject(projectId: string): void {
    generations.set(projectId, (generations.get(projectId) ?? 0) + 1);
    byProject.value = Object.fromEntries(Object.entries(byProject.value).filter(([id]) => id !== projectId));
  }

  return { byProject, items, pending, refresh, acknowledge, removeProject };
}
