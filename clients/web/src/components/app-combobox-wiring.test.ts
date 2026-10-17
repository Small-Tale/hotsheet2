import { describe, expect, it, vi } from 'vitest';

import { TICKET_TAG_COMBOBOX_COMMIT, wireAppComboboxes } from './app-combobox-wiring';

const wiring = vi.hoisted(() => ({
  options: undefined as
    { onCommit: (commit: { id: string; value: string; input: HTMLInputElement }) => void } | undefined,
  dispose: vi.fn(),
}));
vi.mock('@kerfjs/ui/wire-comboboxes', () => ({
  wireComboboxes: (_root: unknown, options: typeof wiring.options) => {
    wiring.options = options;
    return wiring.dispose;
  },
}));

describe('application Combobox routing (HS2-C32ERK)', () => {
  it('clears each tag commit, dispatches it once, and preserves provider repository choices', () => {
    const addEventListener = vi.fn(),
      removeEventListener = vi.fn(),
      root = { addEventListener, removeEventListener } as unknown as HTMLElement;
    const dispose = wireAppComboboxes(root);
    expect(addEventListener).toHaveBeenCalledTimes(1);
    const dispatchEvent = vi.fn(),
      input = { value: 'regression', dispatchEvent } as unknown as HTMLInputElement;
    wiring.options!.onCommit({ id: 'ticket-tag-sidebar-input', value: 'regression', input });
    expect(input.value).toBe('');
    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    const event = dispatchEvent.mock.calls[0][0] as CustomEvent<string>;
    expect(event.type).toBe(TICKET_TAG_COMBOBOX_COMMIT);
    expect(event.bubbles).toBe(true);
    expect(event.detail).toBe('regression');

    wiring.options!.onCommit({ id: 'provider-setup-repository', value: 'team/project', input });
    expect(input.value).toBe('team/project');
    expect(dispatchEvent).toHaveBeenCalledTimes(1);
    dispose();
    expect(wiring.dispose).toHaveBeenCalledOnce();
    expect(removeEventListener).toHaveBeenCalledTimes(1);
  });
});
