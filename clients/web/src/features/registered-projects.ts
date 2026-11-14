import { signal } from 'kerfjs';

import { bridgeErrorMessage } from '../local-bridge-client';
import {
  parseRegisteredProjects,
  REGISTERED_PROJECTS_PATH,
  type RegisteredProject,
  sortRegisteredProjects,
} from '../registered-projects-model';

/** Where App Settings → Projects stands: not shown, loading, or showing the list. */
export type RegisteredProjectsState = 'idle' | 'loading' | 'ready';

const messageOf = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/**
 * App Settings → Projects (HS2-R806FZ): the projects registered with Hot Sheet on this computer.
 * The list loads when the category opens and again after each removal; nothing polls. Removing a
 * project unregisters it through the server's removal workflow (HS2-X4RZ73) and never deletes
 * files. A refusal (for example, the project is open in a window) is shown as the message.
 */
export function createRegisteredProjects(dependencies: { request?: typeof fetch } = {}) {
  const request = dependencies.request ?? ((input, init) => fetch(input, init));
  const list = signal<readonly RegisteredProject[] | undefined>(undefined),
    state = signal<RegisteredProjectsState>('idle'),
    message = signal(''),
    busy = signal<readonly string[]>([]);
  let generation = 0;

  async function load() {
    const current = ++generation;
    try {
      const response = await request(REGISTERED_PROJECTS_PATH, { credentials: 'same-origin' });
      const body: unknown = await response.json().catch(() => undefined);
      if (!response.ok) throw new Error(bridgeErrorMessage(body) ?? 'The Hot Sheet host refused the request.');
      const next = sortRegisteredProjects(parseRegisteredProjects(body));
      if (current !== generation) return;
      list.value = next;
      state.value = 'ready';
      if (message.value.startsWith('Could not load')) message.value = '';
    } catch (reason) {
      if (current !== generation) return;
      state.value = list.value ? 'ready' : 'idle';
      message.value = `Could not load the project list. ${messageOf(reason)}`.trim();
    }
  }

  /** Load the list when the category shows. */
  function start() {
    if (!list.value) state.value = 'loading';
    void load();
  }

  /** Forget an in-flight load when the category hides; the last list stays for the next opening. */
  function stop() {
    generation += 1;
    state.value = list.value ? 'ready' : 'idle';
  }

  /** Unregister one project, then reload the list. Returns whether the host removed it. */
  async function remove(id: string) {
    if (busy.value.includes(id)) return false;
    busy.value = [...busy.value, id];
    message.value = '';
    try {
      const response = await request(`${REGISTERED_PROJECTS_PATH}/${encodeURIComponent(id)}`, {
        method: 'DELETE',
        credentials: 'same-origin',
      });
      if (!response.ok) {
        const answer: unknown = await response.json().catch(() => undefined);
        throw new Error(bridgeErrorMessage(answer) ?? 'The Hot Sheet host refused to remove the project.');
      }
      await load();
      return true;
    } catch (reason) {
      message.value = messageOf(reason);
      return false;
    } finally {
      busy.value = busy.value.filter((item) => item !== id);
    }
  }

  /** Restore the initial state (tests and the catalog demo). */
  function reset() {
    generation += 1;
    list.value = undefined;
    state.value = 'idle';
    message.value = '';
    busy.value = [];
  }

  return { list, state, message, busy, start, stop, remove, reset };
}

export type RegisteredProjects = ReturnType<typeof createRegisteredProjects>;
