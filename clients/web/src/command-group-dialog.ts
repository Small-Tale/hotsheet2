import { signal } from 'kerfjs';

/** The project is captured when creation starts; switching tabs cannot retarget it. */
export interface CommandGroupDialogState {
  projectId: string;
  value: string;
  message: string;
}

export function createCommandGroupDialog(create: (projectId: string, name: string) => string | undefined) {
  const state = signal<CommandGroupDialogState | undefined>(undefined);
  return {
    state,
    open(projectId: string) {
      state.value = { projectId, value: '', message: '' };
    },
    input(value: string) {
      if (state.value) state.value = { ...state.value, value, message: '' };
    },
    cancel() {
      state.value = undefined;
    },
    submit() {
      const current = state.value;
      if (!current) return;
      if (!current.value.trim()) {
        state.value = { ...current, message: 'Enter a group name.' };
        return;
      }
      const message = create(current.projectId, current.value);
      state.value = message ? { ...current, message } : undefined;
    },
  };
}
