import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { AiToolSettings } from './ai-tool-settings';

const tools = [
  {
    id: 'claude',
    display_name: 'Claude',
    models: [
      { id: 'sonnet', label: 'Sonnet', effort_levels: ['low', 'high'] },
      { id: 'opus', label: 'Opus', effort_levels: ['high'] },
    ],
    default_model: 'sonnet',
    default_effort: 'low',
  },
  {
    id: 'codex',
    display_name: 'Codex',
    models: [{ id: 'gpt', label: 'GPT', effort_levels: ['low', 'high'] }],
    default_model: 'gpt',
    default_effort: 'high',
  },
];
/** The `value` a named Select renders with. */
const selectValue = (markup: string, name: string) =>
  new RegExp(`name="${name}"[^>]*value="([^"]*)"`).exec(markup)?.[1];

describe('AiToolSettings (HS2-SW5S13, HS2-EK24KF)', () => {
  it('renders the default provider, then each provider with its own model, efforts, and Other', () => {
    const markup = String(
      AiToolSettings({
        tools,
        selection: { tool: 'codex', model: 'legacy model', providers: { claude: { model: 'opus', effort: 'high' } } },
      }),
    );
    expect(selectValue(markup, 'ai-default-tool')).toBe('codex');
    expect(markup).toContain('label="Default AI provider"');
    // The default provider keeps its custom model; Claude shows its own saved choice.
    expect(selectValue(markup, 'ai-provider-model-codex')).toBe('legacy model');
    expect(selectValue(markup, 'ai-provider-model-claude')).toBe('opus');
    expect(selectValue(markup, 'ai-provider-effort-claude')).toBe('high');
    expect(markup).toContain('<wa-option value="legacy model"');
    expect(markup).toContain('data-ai-provider="codex" data-ai-provider-default="true"');
    expect(markup).toContain('data-ai-provider="claude" data-ai-provider-default="false"');
    expect(markup).toContain('Codex (default)');
    expect(markup).toContain('data-other-model-value="__hotsheet_other_model__"');
    expect(markup.match(/Other…/g)).toHaveLength(2);
    expect(markup).toContain("this project's AI defaults");
    // An unlisted model has no known efforts, so its effort select is disabled.
    expect(markup).toMatch(/name="ai-provider-effort-codex"[^>]*disabled/);
  });
  it('falls back to the manifest defaults for a provider without a saved choice', () => {
    const markup = String(AiToolSettings({ tools, selection: { tool: 'claude', model: 'sonnet', effort: 'high' } }));
    expect(selectValue(markup, 'ai-provider-model-claude')).toBe('sonnet');
    expect(selectValue(markup, 'ai-provider-effort-claude')).toBe('high');
    expect(selectValue(markup, 'ai-provider-model-codex')).toBe('gpt');
    expect(selectValue(markup, 'ai-provider-effort-codex')).toBe('high');
  });
  it('explains the empty detected-plugin state', () => {
    expect(String(AiToolSettings({ tools: [], selection: { tool: '' } }))).toContain('No AI tools detected');
  });
  it('does not carry a stale model across a replaced provider catalog', () => {
    const markup = String(
      AiToolSettings({
        tools: [
          {
            id: 'new',
            display_name: 'New',
            models: [{ id: 'new-default', label: 'New default' }],
            default_model: 'new-default',
          },
        ],
        selection: { tool: 'old', model: 'old-model' },
      }),
    );
    expect(selectValue(markup, 'ai-provider-model-new')).toBe('new-default');
    expect(markup).not.toContain('value="old-model"');
  });
  it('responds to the settings pane width and gives model ids the widest column', () => {
    const css = readFileSync(new URL('./ai-tool-settings.css', import.meta.url), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toContainSource('container-type:inline-size');
    expect(css).toContainSource(
      '.ai-tool-settings { display:grid; container-type:inline-size; gap:var(--kui-space-l); }',
    );
    expect(css).toContainSource('minmax(remify(224px),1.6fr)');
    expect(css).toContainSource('gap:var(--kui-space-m)');
    expect(css).toContainSource('gap:var(--kui-space-xs)');
    expect(css).toContain('@container (max-width:remify(416px))');
  });
});
