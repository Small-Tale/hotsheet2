export type LiveControl = HTMLElement & { checked: boolean; value: string };

export interface ControlState {
  values?: Record<string, string>;
  checked?: Record<string, boolean>;
}

/** Synchronize state into live custom-element properties after a Kerf morph/reset. */
export function syncSettingsControls(root: ParentNode, settingsId: string, state: ControlState): void {
  const control = (name: string) => root.querySelector(`[data-settings="${settingsId}"] [name="${name}"]`) as LiveControl | null;
  for (const [name, value] of Object.entries(state.values ?? {})) {
    const item = control(name);
    if (item) item.value = value;
  }
  for (const [name, checked] of Object.entries(state.checked ?? {})) {
    const item = control(name);
    if (item) item.checked = checked;
  }
}
