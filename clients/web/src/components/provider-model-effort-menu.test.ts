import { describe, expect, it } from 'vitest';

import { ProviderModelEffortSubmenus } from './provider-model-effort-menu';

const providers = { choices: [{ id: 'codex', label: 'Codex' }, { id: 'claude', label: 'Claude' }], currentId: 'codex', currentLabel: 'Codex' };
const model = { choices: [{ id: 'gpt-5.6', label: 'GPT-5.6' }, { id: 'gpt-5.7', label: 'GPT-5.7' }], currentId: 'gpt-5.6', currentLabel: 'GPT-5.6' };
const actions = { provider: 'select-x-provider', model: 'select-x-model', effort: 'select-x-effort', manualModel: 'open-x-manual-model' };

describe('ProviderModelEffortSubmenus', () => {
  it('renders every supplied submenu with its action, selection marker, and the Other entry', () => {
    const markup = String(ProviderModelEffortSubmenus({ actions, providers, model, effort: { efforts: ['medium', 'high'], current: 'high' } }));
    expect(markup).toContain('data-action="select-x-provider" data-value="claude"');
    expect(markup).toContain('data-action="select-x-model" data-value="gpt-5.7"');
    expect(markup).toContain('data-action="select-x-effort" data-value="medium"');
    expect(markup).toContain('data-action="open-x-manual-model"');
    // Current provider, model, and effort each carry exactly one selection marker.
    expect(markup.match(/aria-current="true"/g)).toHaveLength(3);
  });

  it('shows an ephemeral custom model as its own selected row before the catalog', () => {
    const markup = String(ProviderModelEffortSubmenus({ actions, model: { ...model, currentId: undefined, currentLabel: 'legacy model', customModel: 'legacy model' } }));
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
    const noProviderAction = String(ProviderModelEffortSubmenus({ actions: { ...actions, provider: undefined }, providers, model }));
    expect(noProviderAction).not.toContain('data-value="claude"');
  });

  it('disables the Effort item when the model declares no effort levels but keeps it present', () => {
    const disabled = String(ProviderModelEffortSubmenus({ actions, model, effort: { efforts: [], current: undefined } }));
    expect(disabled).toContain('disabled');
    expect(disabled).toContain('Effort');
    const enabled = String(ProviderModelEffortSubmenus({ actions, model, effort: { efforts: ['low'], current: 'low' } }));
    expect(enabled).not.toContain('disabled');
  });
});
