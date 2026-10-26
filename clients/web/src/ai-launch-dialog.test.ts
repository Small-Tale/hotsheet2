import { describe, expect, it } from 'vitest';

import { createAiLaunchDialog } from './ai-launch-dialog';
import { providerSelection } from './ai-provider-defaults';
import type { AiToolDescriptor } from './api';
import { AiLaunchDialog } from './components/ai-launch-dialog';

const tools: AiToolDescriptor[] = [
  {
    id: 'fixture',
    display_name: 'Fixture AI',
    models: [{ id: 'one', label: 'One', effort_levels: ['low', 'high'] }],
    default_model: 'one',
    default_effort: 'low',
  },
  {
    id: 'other',
    display_name: 'Other AI',
    models: [{ id: 'two', label: 'Two', effort_levels: ['medium'] }],
    default_model: 'two',
    default_effort: 'medium',
  },
];
const create = () =>
  createAiLaunchDialog({
    tools: () => tools,
    normalize: (selection) =>
      providerSelection({ tool: selection.tool ?? 'fixture', ...selection }, tools, selection.tool ?? 'fixture'),
  });

describe('AI launch dialog (HS2-WMZ6Z5)', () => {
  it('retains initial values, resolves cancellation, and replaces an outstanding request without launching it', async () => {
    const dialog = create();
    dialog.submit();
    dialog.input('model', 'ignored');
    const first = dialog.request('a', 'ai-chat', { tool: 'fixture', model: 'one', effort: 'high' });
    expect(dialog.state.value).toMatchObject({ model: 'one', effort: 'high', effortLevels: ['low', 'high'] });
    const second = dialog.request('b', 'ai-shell', { tool: 'other' });
    expect(await first).toBeUndefined();
    expect(String(AiLaunchDialog({ state: dialog.state.value }))).toContain('New AI Terminal');
    dialog.cancel();
    expect(await second).toBeUndefined();
    expect(dialog.state.value).toBeUndefined();
    expect(AiLaunchDialog({})).toBeNull();
  });

  it('keeps invalid input editable and blank fields select the chosen provider defaults', async () => {
    const dialog = create();
    const result = dialog.request('a', 'ai-chat', { tool: 'fixture', model: 'one' });
    dialog.input('provider', 'unknown');
    dialog.submit();
    expect(dialog.state.value?.message).toBe('Unknown AI provider: unknown');
    dialog.input('provider', 'OTHER AI');
    expect(dialog.state.value).toMatchObject({ model: 'two', effort: 'medium', message: '' });
    dialog.input('model', '  ');
    dialog.input('effort', '');
    dialog.submit();
    expect(await result).toEqual({ tool: 'other', model: 'two', effort: 'medium' });
    expect(dialog.state.value).toBeUndefined();
    const custom = dialog.request('a', 'ai-chat', { tool: 'fixture' });
    dialog.input('model', '  exact-custom-id  ');
    dialog.input('effort', ' high ');
    dialog.submit();
    const customSelection = await custom;
    expect(customSelection).toMatchObject({ tool: 'fixture', model: 'exact-custom-id' });
    expect(customSelection).not.toHaveProperty('effort');
  });
});
