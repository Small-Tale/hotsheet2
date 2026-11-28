import { delegate, signal } from 'kerfjs';

import type { Api } from '../api';
import type { ProjectTimeZoneSettingProps } from '../components/project-time-zone-setting';
import { PROJECT_TIME_ZONE_FIELD } from '../project-time-zone';

type TimeZoneClient = Pick<Api, 'timeZoneSettings' | 'saveTimeZoneSettings'>;

export interface ProjectTimeZoneState {
  timeZone?: string;
  diagnostic?: string;
  message?: string;
  loading?: boolean;
}

export interface ProjectTimeZoneSettingsDependencies {
  /** The client for a project, or none when it is no longer the open project. */
  client: (projectId: string, quiet: boolean) => TimeZoneClient | undefined;
}

const failureMessage = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/** The Time Zone Select inside Project Settings › General. */
export const PROJECT_TIME_ZONE_SELECT = `[data-component="project-time-zone-setting"] [name="${PROJECT_TIME_ZONE_FIELD}"]`;

/**
 * Project Settings › General › Time Zone (HS2-SZF152): reads and saves the project's shared `time_zone`
 * through `/checkouts/{id}/time-zone-settings`. A choice shows at once and saves without a Save button; a
 * refusal restores the zone that was in effect and names the problem under the control. Only the newest
 * read or save of a project applies, so an older answer arriving late never overwrites a newer choice.
 */
export function createProjectTimeZoneSettings({ client }: ProjectTimeZoneSettingsDependencies) {
  const states = signal<Record<string, ProjectTimeZoneState>>({});
  const generations = new Map<string, number>();

  function patch(projectId: string, next: ProjectTimeZoneState) {
    states.value = { ...states.value, [projectId]: { ...states.value[projectId], ...next } };
  }
  function begin(projectId: string) {
    const generation = (generations.get(projectId) ?? 0) + 1;
    generations.set(projectId, generation);
    return () => generations.get(projectId) === generation;
  }

  async function refresh(projectId: string, quiet = false) {
    const api = client(projectId, quiet);
    if (!api) return;
    const current = begin(projectId);
    patch(projectId, { loading: true });
    try {
      const settings = await api.timeZoneSettings(projectId);
      if (!current()) return;
      patch(projectId, {
        timeZone: settings.time_zone,
        diagnostic: settings.diagnostic ?? '',
        message: '',
        loading: false,
      });
    } catch (reason) {
      if (current()) patch(projectId, { message: failureMessage(reason), loading: false });
    }
  }

  async function save(projectId: string, timeZone: string) {
    const api = client(projectId, false);
    const state = states.value[projectId] ?? {};
    if (!api || (timeZone === state.timeZone && !state.diagnostic && !state.message)) return;
    const current = begin(projectId);
    const previous = state.timeZone;
    patch(projectId, { timeZone, message: '', loading: false });
    try {
      const saved = await api.saveTimeZoneSettings(projectId, timeZone);
      if (current()) patch(projectId, { timeZone: saved.time_zone, diagnostic: saved.diagnostic ?? '' });
    } catch (reason) {
      if (current()) patch(projectId, { timeZone: previous, message: failureMessage(reason) });
    }
  }

  function view(projectId: string): ProjectTimeZoneSettingProps {
    const state = states.value[projectId] ?? {};
    return {
      timeZone: state.timeZone,
      loading: state.loading ?? state.timeZone === undefined,
      message: state.message ?? '',
      diagnostic: state.diagnostic ?? '',
    };
  }

  function reset() {
    generations.clear();
    states.value = {};
  }

  return { states, refresh, save, view, reset };
}

export type ProjectTimeZoneSettings = ReturnType<typeof createProjectTimeZoneSettings>;

/**
 * Save the Time Zone Select's choice under `root`. The application and the catalog demo install the same
 * handler over the same controller; returns the disposer.
 */
export function wireProjectTimeZoneSetting(
  root: HTMLElement,
  settings: ProjectTimeZoneSettings,
  projectId: () => string | undefined,
): () => void {
  return delegate(root, 'change', PROJECT_TIME_ZONE_SELECT, (_event, target) => {
    const id = projectId(),
      value = (target as HTMLElement & { value?: string }).value;
    if (id && value) void settings.save(id, value);
  });
}
