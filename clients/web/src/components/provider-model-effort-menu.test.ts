import type { PopupMenuItem } from '@kerfjs/ui/popup-menu';
import { describe, expect, it } from 'vitest';

import { providerModelEffortEntries, ProviderModelEffortSubmenus } from './provider-model-effort-menu';

const providers = {
  choices: [
    { id: 'codex', label: 'Codex' },
    { id: 'claude', label: 'Claude' },
  ],
  currentId: 'codex',
  currentLabel: 'Codex',
};
const model = {
  choices: [
    { id: 'gpt-5.6', label: 'GPT-5.6' },
    { id: 'gpt-5.7', label: 'GPT-5.7' },
  ],
  currentId: 'gpt-5.6',
  currentLabel: 'GPT-5.6',
};
const actions = {
  provider: 'select-x-provider',
  model: 'select-x-model',
  effort: 'select-x-effort',
  manualModel: 'open-x-manual-model',
};

describe('ProviderModelEffortSubmenus', () => {
  it('renders every supplied submenu with its action, selection marker, and the Other entry', () => {
    const markup = String(
      ProviderModelEffortSubmenus({
        actions,
        providers,
        model,
        effort: { efforts: ['medium', 'high'], current: 'high' },
      }),
    );
    expect(markup).toContain('data-action="select-x-provider" data-value="claude"');
    expect(markup).toContain('data-action="select-x-model" data-value="gpt-5.7"');
    expect(markup).toContain('data-action="select-x-effort" data-value="medium"');
    expect(markup).toContain('data-action="open-x-manual-model"');
    // Current provider, model, and effort each carry exactly one selection marker.
    expect(markup.match(/aria-current="true"/g)).toHaveLength(3);
  });

  it('shows an ephemeral custom model as its own selected row before the catalog', () => {
    const markup = String(
      ProviderModelEffortSubmenus({
        actions,
        model: { ...model, currentId: undefined, currentLabel: 'legacy model', customModel: 'legacy model' },
      }),
    );
    const custom = markup.indexOf('data-value="legacy model"');
    const catalog = markup.indexOf('data-value="gpt-5.6"');
    expect(custom).toBeGreaterThanOrEqual(0);
    expect(custom).toBeLessThan(catalog);
    expect(markup).toContain('aria-current="true" data-action="select-x-model" data-value="legacy model"');
  });

  it('omits a submenu whose data is not supplied', () => {
    const modelOnly = String(ProviderModelEffortSubmenus({ actions, model }));
    expect(modelOnly).not.toContain('select-x-provider');
    expect(modelOnly).not.toContain('select-x-effort');
    expect(modelOnly).toContain('select-x-model');
    // Provider submenu also needs an action, not just data.
    const noProviderAction = String(
      ProviderModelEffortSubmenus({ actions: { ...actions, provider: undefined }, providers, model }),
    );
    expect(noProviderAction).not.toContain('data-value="claude"');
  });

  it('disables the Effort item when the model declares no effort levels but keeps it present', () => {
    const disabled = String(
      ProviderModelEffortSubmenus({ actions, model, effort: { efforts: [], current: undefined } }),
    );
    expect(disabled).toContain('disabled');
    expect(disabled).toContain('Effort');
    const enabled = String(
      ProviderModelEffortSubmenus({ actions, model, effort: { efforts: ['low'], current: 'low' } }),
    );
    expect(enabled).not.toContain('disabled');
  });
});

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
