import { describe, expect, it } from 'vitest';

import {
  compatiblePermissionMode,
  providerSelection,
  withDefaultProvider,
  withProviderSelection,
} from './ai-provider-defaults';
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

describe('per-provider permission modes (HS2-NHRDMW)', () => {
  const moded: AiToolDescriptor[] = [
    {
      ...tools[0],
      permission_modes: [
        { id: 'acceptEdits', label: 'Accept edits' },
        { id: 'auto', label: 'Auto' },
        { id: 'bypassPermissions', label: 'Bypass permissions', warning: 'Runs everything.' },
      ],
      default_permission_mode: 'acceptEdits',
    },
    {
      ...tools[1],
      permission_modes: [
        { id: 'never', label: 'Never ask' },
        { id: 'on-request', label: 'Ask when needed' },
      ],
      default_permission_mode: 'never',
    },
  ];

  it('accepts only a declared mode, else the declared default, and none for a tool without modes', () => {
    expect(compatiblePermissionMode(moded[0], 'auto')).toBe('auto');
    // A Claude value is not a Codex approval policy.
    expect(compatiblePermissionMode(moded[1], 'acceptEdits')).toBe('never');
    expect(compatiblePermissionMode(moded[1], undefined, 'on-request')).toBe('on-request');
    expect(compatiblePermissionMode(tools[0], 'auto')).toBeUndefined();
    expect(compatiblePermissionMode(undefined, 'auto')).toBeUndefined();
  });

  it('resolves the provider entry, then the default selection, then the manifest default', () => {
    const defaults: AiToolDefaults = {
      tool: 'claude',
      model: 'sonnet',
      permission_mode: 'auto',
      providers: { codex: { model: 'gpt', permission_mode: 'on-request' } },
    };
    expect(providerSelection(defaults, moded, 'claude').permission_mode).toBe('auto');
    expect(providerSelection(defaults, moded, 'codex').permission_mode).toBe('on-request');
    expect(providerSelection({ tool: 'claude' }, moded, 'codex').permission_mode).toBe('never');
    // The selection for a tool without modes carries none.
    expect(providerSelection(defaults, tools, 'claude')).not.toHaveProperty('permission_mode');
  });

  it('keeps a saved mode across model changes and never leaks the old default mode to a new default', () => {
    let defaults: AiToolDefaults = { tool: 'claude', model: 'sonnet', effort: 'low' };
    defaults = withProviderSelection(defaults, 'claude', { model: 'sonnet', effort: 'low', permission_mode: 'auto' });
    expect(defaults.permission_mode).toBe('auto');
    expect(defaults.providers?.claude.permission_mode).toBe('auto');
    // A model change without a mode keeps the saved mode (default and non-default providers).
    defaults = withProviderSelection(defaults, 'claude', { model: 'opus', effort: 'high' });
    expect(defaults.permission_mode).toBe('auto');
    defaults = withProviderSelection(defaults, 'codex', { model: 'gpt', permission_mode: 'on-request' });
    defaults = withProviderSelection(defaults, 'codex', { model: 'gpt', effort: 'xhigh' });
    expect(defaults.providers?.codex.permission_mode).toBe('on-request');
    // Switching the default provider carries each provider's own mode, never the outgoing one's.
    const switched = withDefaultProvider(defaults, moded, 'codex');
    expect(switched.permission_mode).toBe('on-request');
    expect(switched.providers?.claude.permission_mode).toBe('auto');
    const back = withDefaultProvider(switched, moded, 'claude');
    expect(back.permission_mode).toBe('auto');
    // A provider that never picked a mode stays implicit, so a later plugin default still applies.
    const fresh = withDefaultProvider({ tool: 'claude', permission_mode: 'auto' }, moded, 'codex');
    expect(fresh.permission_mode).toBeUndefined();
    expect(fresh.providers?.claude.permission_mode).toBe('auto');
  });
});
