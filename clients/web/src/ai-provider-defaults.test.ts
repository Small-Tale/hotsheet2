import { describe, expect, it } from 'vitest';

import { providerSelection, withDefaultProvider, withProviderSelection } from './ai-provider-defaults';
import type { AiToolDefaults } from './api';
import type { AiToolDescriptor } from './components/drive-options-menu';

const tools: AiToolDescriptor[] = [
  {
    id: 'claude',
    display_name: 'Claude',
    models: [
      { id: 'sonnet', label: 'Sonnet', effort_levels: ['low', 'medium', 'high'] },
      { id: 'opus', label: 'Opus', effort_levels: ['high'] },
    ],
    default_model: 'sonnet',
    default_effort: 'medium',
  },
  {
    id: 'codex',
    display_name: 'Codex',
    models: [{ id: 'gpt', label: 'GPT', effort_levels: ['low', 'xhigh'] }],
    default_model: 'gpt',
    default_effort: 'low',
  },
];

describe('per-provider AI defaults (HS2-EK24KF)', () => {
  it('resolves each provider from its own entry, the default selection, then the manifest', () => {
    const defaults: AiToolDefaults = {
      tool: 'claude',
      model: 'opus',
      effort: 'high',
      providers: { codex: { model: 'gpt', effort: 'xhigh' } },
    };
    expect(providerSelection(defaults, tools, 'claude')).toEqual({ tool: 'claude', model: 'opus', effort: 'high' });
    expect(providerSelection(defaults, tools, 'codex')).toEqual({ tool: 'codex', model: 'gpt', effort: 'xhigh' });
    // No entry: the manifest default model and effort.
    expect(providerSelection({ tool: 'claude' }, tools, 'codex')).toEqual({
      tool: 'codex',
      model: 'gpt',
      effort: 'low',
    });
    // An effort the model does not support falls back to the manifest default, then the first level.
    expect(
      providerSelection({ tool: 'codex', providers: { claude: { model: 'opus', effort: 'low' } } }, tools, 'claude'),
    ).toEqual({ tool: 'claude', model: 'opus', effort: 'high' });
    // A custom (unlisted) model keeps its effort.
    expect(
      providerSelection({ tool: 'claude', providers: { codex: { model: 'custom', effort: 'turbo' } } }, tools, 'codex'),
    ).toEqual({ tool: 'codex', model: 'custom', effort: 'turbo' });
  });

  it('saves a non-default provider without touching the default, and mirrors the default provider', () => {
    const base: AiToolDefaults = { tool: 'claude', model: 'sonnet', effort: 'medium' };
    const codex = withProviderSelection(base, 'codex', { model: 'gpt', effort: 'xhigh' });
    expect(codex).toEqual({ ...base, providers: { codex: { model: 'gpt', effort: 'xhigh' } } });
    const claude = withProviderSelection(codex, 'claude', { model: 'opus', effort: 'high' });
    expect(claude).toEqual({
      tool: 'claude',
      model: 'opus',
      effort: 'high',
      providers: { codex: { model: 'gpt', effort: 'xhigh' }, claude: { model: 'opus', effort: 'high' } },
    });
  });

  it('switches the default provider back and forth without losing either choice', () => {
    let defaults: AiToolDefaults = {
      tool: 'claude',
      model: 'opus',
      effort: 'high',
      providers: { codex: { model: 'gpt', effort: 'xhigh' } },
    };
    defaults = withDefaultProvider(defaults, tools, 'codex');
    expect(defaults).toMatchObject({ tool: 'codex', model: 'gpt', effort: 'xhigh' });
    expect(defaults.providers?.claude).toEqual({ model: 'opus', effort: 'high' });
    defaults = withDefaultProvider(defaults, tools, 'claude');
    expect(defaults).toMatchObject({ tool: 'claude', model: 'opus', effort: 'high' });
    expect(defaults.providers?.codex).toEqual({ model: 'gpt', effort: 'xhigh' });
    // A legacy value with no providers map switches cleanly to the manifest defaults.
    expect(withDefaultProvider({ tool: 'claude', model: 'opus', effort: 'high' }, tools, 'codex')).toMatchObject({
      tool: 'codex',
      model: 'gpt',
      effort: 'low',
    });
  });
});
