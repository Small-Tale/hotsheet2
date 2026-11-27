import { closeDialog, openDialog } from '@kerfjs/ui/surface-scaffold';
import { signal } from 'kerfjs';

import { Api } from '../api';
import {
  newScheduleDraft,
  scheduleDraftFromView,
  type ScheduleFormDraft,
  scheduleInputFromDraft,
  withScheduleField,
} from '../command-schedule-form';
import type { ScheduleView } from '../command-schedules';
import { COMMAND_SCHEDULE_DIALOG_ID } from '../components/command-schedule-dialog';
import type { Project } from '../interactions/types';

/** One controller for schedule form state and its checkout-local API (HS2-17663F). */
export function createCommandSchedulesController(project: () => Project | undefined) {
  const viewsByProject = signal<Partial<Record<string, ScheduleView[]>>>({});
  const openCommandId = signal<string | undefined>(undefined);
  const draft = signal<ScheduleFormDraft | undefined>(undefined);
  const preview = signal<string[]>([]);
  const previewBusy = signal(false);
  const busy = signal(false);
  const error = signal('');
  let generation = 0;
  let previewGeneration = 0;
  const currentView = () => {
    const current = project();
    return current && openCommandId.value
      ? viewsByProject.value[current.id]?.find((item) => item.schedule.command_id === openCommandId.value)
      : undefined;
  };
  async function refresh(current = project(), quiet = false) {
    if (!current) return;
    const request = ++generation;
    try {
      const views = await new Api(current.apiPath, '', { trackBusy: !quiet }).commandSchedules(current.id);
      if (request !== generation || project()?.id !== current.id) return;
      viewsByProject.value = { ...viewsByProject.value, [current.id]: views };
    } catch (reason) {
      if (request !== generation || project()?.id !== current.id) return;
      // Older hosts may not have the scheduler. Keep an existing successful read visible.
      if (viewsByProject.value[current.id] === undefined)
        viewsByProject.value = { ...viewsByProject.value, [current.id]: [] };
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  async function refreshPreview() {
    const current = project(),
      value = draft.value;
    if (!current || !value) return;
    const request = ++previewGeneration;
    previewBusy.value = true;
    try {
      const result = await new Api(current.apiPath).previewCommandSchedule(current.id, scheduleInputFromDraft(value));
      if (request === previewGeneration && project()?.id === current.id) preview.value = result.next_runs;
    } catch {
      if (request === previewGeneration) preview.value = [];
    } finally {
      if (request === previewGeneration) previewBusy.value = false;
    }
  }
  function open(commandId: string) {
    const current = project();
    if (!current) return;
    const saved = viewsByProject.value[current.id]?.find((item) => item.schedule.command_id === commandId);
    openCommandId.value = commandId;
    draft.value = saved ? scheduleDraftFromView(saved) : newScheduleDraft();
    preview.value = saved?.next_runs ?? [];
    error.value = '';
    const dialog = document.querySelector<HTMLDialogElement>(`#${COMMAND_SCHEDULE_DIALOG_ID}`);
    if (dialog) openDialog(dialog);
    if (!saved) void refreshPreview();
  }
  function close() {
    const dialog = document.querySelector<HTMLDialogElement>(`#${COMMAND_SCHEDULE_DIALOG_ID}`);
    if (dialog) closeDialog(dialog);
    openCommandId.value = undefined;
    draft.value = undefined;
    error.value = '';
    previewGeneration++;
  }
  function change(name: string, value: string | string[]) {
    const previous = draft.value;
    if (!previous || !(name in previous)) return;
    // HS2-SEEMXR: the recurrence fields change the draft as the Triggers… setup changes its own.
    if (name === 'action') draft.value = { ...previous, action: value as ScheduleFormDraft['action'] };
    else draft.value = withScheduleField(previous, name, value);
    error.value = '';
    void refreshPreview();
  }
  async function save() {
    const current = project(),
      commandId = openCommandId.value,
      value = draft.value;
    if (!current || !commandId || !value || busy.value) return;
    if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(value.time)) {
      error.value = 'Enter time as HH:mm in 24-hour format.';
      return;
    }
    busy.value = true;
    error.value = '';
    try {
      const saved = await new Api(current.apiPath).saveCommandSchedule(
        current.id,
        commandId,
        scheduleInputFromDraft(value),
      );
      const before = viewsByProject.value[current.id] ?? [];
      viewsByProject.value = {
        ...viewsByProject.value,
        [current.id]: [...before.filter((item) => item.schedule.command_id !== commandId), saved],
      };
      close();
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busy.value = false;
    }
  }
  async function remove() {
    const current = project(),
      commandId = openCommandId.value;
    if (!current || !commandId || busy.value) return;
    busy.value = true;
    error.value = '';
    try {
      await new Api(current.apiPath).deleteCommandSchedule(current.id, commandId);
      viewsByProject.value = {
        ...viewsByProject.value,
        [current.id]: (viewsByProject.value[current.id] ?? []).filter((item) => item.schedule.command_id !== commandId),
      };
      close();
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busy.value = false;
    }
  }
  async function togglePaused() {
    const value = draft.value;
    if (!value) return;
    draft.value = { ...value, paused: !value.paused };
    await save();
  }
  return {
    viewsByProject,
    openCommandId,
    draft,
    preview,
    previewBusy,
    busy,
    error,
    currentView,
    refresh,
    open,
    close,
    change,
    save,
    remove,
    togglePaused,
  };
}
