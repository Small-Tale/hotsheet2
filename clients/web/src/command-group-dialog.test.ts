import { describe, expect, it, vi } from 'vitest';

import { createCommandGroupDialog } from './command-group-dialog';
import { CommandGroupDialog } from './components/command-group-dialog';

describe('command group creation (HS2-WMZ6Z5)', () => {
  it('captures the project, rejects empty/duplicate names, then resets on cancel and reopen', () => {
    const create = vi.fn((_project: string, name: string) =>
      name.trim() === 'Existing' ? 'Already exists.' : undefined,
    );
    const dialog = createCommandGroupDialog(create);
    dialog.submit();
    dialog.open('a');
    dialog.input('   ');
    dialog.submit();
    expect(create).not.toHaveBeenCalled();
    expect(dialog.state.value?.message).toBe('Enter a group name.');
    dialog.input('Existing');
    dialog.submit();
    expect(dialog.state.value?.message).toBe('Already exists.');
    expect(String(CommandGroupDialog({ state: dialog.state.value }))).toContain('value="Existing"');
    dialog.cancel();
    expect(dialog.state.value).toBeUndefined();
    dialog.open('b');
    expect(dialog.state.value).toEqual({ projectId: 'b', value: '', message: '' });
    dialog.input('New');
    dialog.submit();
    expect(create).toHaveBeenLastCalledWith('b', 'New');
    expect(dialog.state.value).toBeUndefined();
    expect(CommandGroupDialog({})).toBeNull();
  });
});
