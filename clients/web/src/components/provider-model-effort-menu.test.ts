import type { PopupMenuItem } from '@kerfjs/ui/popup-menu';
import { describe, expect, it } from 'vitest';

import { providerModelEffortEntries } from './provider-model-effort-menu';

describe('providerModelEffortEntries (PopupMenu form, HS2-CSRJ9Y)', () => {
  const props = {
    actions: { provider: 'pick-provider', model: 'pick-model', effort: 'pick-effort', manualModel: 'manual-model' },
    providers: {
      choices: [
        { id: 'codex', label: 'Codex' },
        { id: 'claude', label: 'Claude' },
      ],
      currentId: 'codex',
      currentLabel: 'Codex',
    },
    model: {
      choices: [{ id: 'gpt', label: 'GPT' }],
      currentId: undefined,
      currentLabel: 'legacy',
      customModel: 'legacy',
    },
    effort: { efforts: ['low', 'high'], current: 'high' },
  };

  it('mirrors the JSX submenus as typed entries with delegated actions, values, and checked choices', () => {
    const entries = providerModelEffortEntries(props);
    expect(entries.map((entry) => ('label' in entry ? entry.label : entry.kind))).toEqual([
      'Provider',
      'Model',
      'Effort',
    ]);
    const [provider, model, effort] = entries as Array<PopupMenuItem & { submenu: PopupMenuItem[] }>;
    expect(provider.submenu.map((item) => [item.action, item.value, item.checked])).toEqual([
      ['pick-provider', 'codex', true],
      ['pick-provider', 'claude', false],
    ]);
    expect(provider.submenu[0].attributes).toEqual({ 'data-value': 'codex' });
    // The custom model leads as its own checked row, then the catalog, then Other….
    expect(model.submenu.map((item) => [item.action, item.value, item.checked])).toEqual([
      ['pick-model', 'legacy', true],
      ['pick-model', 'gpt', false],
      ['manual-model', 'other', undefined],
    ]);
    expect(effort.submenu.map((item) => [item.value, item.checked])).toEqual([
      ['low', false],
      ['high', true],
    ]);
    expect(effort.disabled).toBe(false);
  });

  it('omits unsupplied submenus and disables Effort without levels, like the JSX form', () => {
    const entries = providerModelEffortEntries({ ...props, providers: undefined, effort: { efforts: [] } });
    expect(entries.map((entry) => ('label' in entry ? entry.label : entry.kind))).toEqual(['Model', 'Effort']);
    expect((entries[1] as PopupMenuItem).disabled).toBe(true);
  });
});
