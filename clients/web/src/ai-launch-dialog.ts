import { signal } from 'kerfjs';

import type { AiToolDefaults, AiToolDescriptor } from './api';

export interface AiLaunchDialogState {
  projectId: string;
  kind: 'ai-shell' | 'ai-chat';
  provider: string;
  model: string;
  effort: string;
  effortLevels: readonly string[];
  message: string;
}

/** One pending launch, independent of the project defaults and other AI editors (HS2-WMZ6Z5). */
export function createAiLaunchDialog(options: {
  tools: () => readonly AiToolDescriptor[];
  normalize: (selection: Partial<AiToolDefaults>) => AiToolDefaults;
}) {
  const state = signal<AiLaunchDialogState | undefined>(undefined);
  let resolve: ((selection: AiToolDefaults | undefined) => void) | undefined;
  function effortLevels(provider: string, model: string) {
    const descriptor = options
      .tools()
      .find(
        (tool) =>
          tool.id.toLowerCase() === provider.trim().toLowerCase() ||
          tool.display_name.toLowerCase() === provider.trim().toLowerCase(),
      );
    const id = model.trim() || (descriptor ? options.normalize({ tool: descriptor.id }).model : undefined);
    return descriptor?.models.find((item) => item.id === id)?.effort_levels ?? [];
  }

  function cancel() {
    state.value = undefined;
    const done = resolve;
    resolve = undefined;
    done?.(undefined);
  }
  return {
    state,
    cancel,
    request(projectId: string, kind: AiLaunchDialogState['kind'], initial: AiToolDefaults) {
      cancel();
      const levels = effortLevels(initial.tool, initial.model ?? '');
      state.value = {
        projectId,
        kind,
        provider: initial.tool,
        model: initial.model ?? '',
        effort: initial.effort && levels.includes(initial.effort) ? initial.effort : '',
        effortLevels: levels,
        message: '',
      };
      return new Promise<AiToolDefaults | undefined>((done) => {
        resolve = done;
      });
    },
    input(field: 'provider' | 'model' | 'effort', value: string) {
      const current = state.value;
      if (!current) return;
      if (field === 'effort') value = value.trim();
      const descriptor = options
        .tools()
        .find(
          (tool) =>
            tool.id.toLowerCase() === value.trim().toLowerCase() ||
            tool.display_name.toLowerCase() === value.trim().toLowerCase(),
        );
      const initial = field === 'provider' && descriptor ? options.normalize({ tool: descriptor.id }) : undefined;
      const next = {
        ...current,
        ...(initial ? { model: initial.model ?? '', effort: initial.effort ?? '' } : {}),
        [field]: value,
        message: '',
      };
      next.effortLevels = effortLevels(next.provider, next.model);
      if (next.effort && !next.effortLevels.includes(next.effort.trim())) next.effort = '';
      state.value = next;
    },
    submit() {
      const current = state.value;
      if (!current) return;
      const descriptor = options
        .tools()
        .find(
          (tool) =>
            tool.id.toLowerCase() === current.provider.trim().toLowerCase() ||
            tool.display_name.toLowerCase() === current.provider.trim().toLowerCase(),
        );
      if (!descriptor) {
        state.value = { ...current, message: `Unknown AI provider: ${current.provider}` };
        return;
      }
      const selection = options.normalize({
        tool: descriptor.id,
        model: current.model.trim() || undefined,
        effort: current.effort.trim() || undefined,
      });
      state.value = undefined;
      const done = resolve;
      resolve = undefined;
      done?.(selection);
    },
  };
}
