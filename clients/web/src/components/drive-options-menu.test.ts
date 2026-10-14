import { existsSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { DriveOptionsMenu } from './drive-options-menu';

const tools = [
  {
    id: 'codex',
    display_name: 'Codex',
    default_model: 'gpt-5.6',
    default_effort: 'high',
    models: [{ id: 'gpt-5.6', label: 'GPT-5.6', effort_levels: ['medium', 'high'] }],
  },
  { id: 'claude', display_name: 'Claude', models: [{ id: 'opus', label: 'Opus' }] },
];

describe('DriveOptionsMenu', () => {
  it('renders plugin-discovered submenus plus a selected ephemeral custom model and Other action', () => {
    const markup = String(
      DriveOptionsMenu({
        tools,
        selection: { tool: 'codex', model: 'legacy model', effort: 'high' },
        defaultSelection: { tool: 'claude', model: 'opus' },
      }),
    );
    expect(markup).toContain('aria-label="Drive Provider, Model, and Effort Options"');
    expect(markup).toContain('data-action="select-drive-default" type="checkbox"');
    expect(markup).toContain(
      'data-value="claude" slot="submenu" data-action="select-drive-tool" value="claude" type="checkbox"',
    );
    expect(markup).toContain(
      'data-value="gpt-5.6" slot="submenu" data-action="select-drive-model" value="gpt-5.6" type="checkbox"',
    );
    expect(markup).toContain(
      'data-value="legacy model" slot="submenu" data-action="select-drive-model" value="legacy model" type="checkbox" checked',
    );
    expect(markup).toContain('data-action="open-drive-manual-model"');
    expect(markup).not.toContain('name="drive-model"');
    // The current provider and the ephemeral custom model are checked PopupMenu choices (HS2-2EHD8R).
    expect(markup.match(/type="checkbox" checked/g)).toHaveLength(2);
  });
  it('keeps Other available while disabling effort when a plugin exposes no models', () => {
    const markup = String(
      DriveOptionsMenu({
        tools: [{ id: 'plain', display_name: 'Plain', models: [] }],
        selection: { tool: 'plain' },
        defaultSelection: { tool: 'plain' },
      }),
    );
    expect(markup).toContain('data-action="open-drive-manual-model"');
    expect(markup.match(/disabled/g)?.length).toBeGreaterThanOrEqual(1);
  });
  it('distinguishes active discovery and a discovery error from a confirmed empty catalog', () => {
    const loading = String(DriveOptionsMenu({ tools: [], selection: {}, defaultSelection: {}, loading: true })),
      failed = String(
        DriveOptionsMenu({ tools: [], selection: {}, defaultSelection: {}, error: 'AI discovery unavailable' }),
      );
    expect(loading).toContain('Detecting AI tools…');
    expect(loading).not.toContain('No AI Tools Detected');
    expect(failed).toContain('AI discovery unavailable');
    expect(failed).not.toContain('No AI Tools Detected');
  });
  it('uses the shared icon-label selection hierarchy without a duplicate disclosure icon', () => {
    const markup = String(
      DriveOptionsMenu({
        tools,
        selection: { tool: 'codex', model: 'gpt-5.6', effort: 'high' },
        defaultSelection: { tool: 'claude', model: 'opus' },
      }),
    );
    expect(markup).toContain('data-context-menu="drive-options"');
    expect(markup.match(/data-lucide="bot"/g)).toHaveLength(3);
    expect(markup.match(/data-lucide="brain"/g)).toHaveLength(2);
    expect(markup.match(/data-lucide="gauge"/g)).toHaveLength(3);
    expect(
      markup.match(/slot="submenu" data-action="select-drive-[a-z]+" value="[^"]+" type="checkbox"/g)?.length,
    ).toBeGreaterThan(0);
    // Kerf's context PopupMenu renders its own anchor (KF-FXGW0C, HS2-H4JZDQ); the menu has no stylesheet.
    expect(markup).toContain('data-context-menu="drive-options"');
    expect(markup).not.toContain('context-menu-anchor');
    expect(existsSync(new URL('./drive-options-menu.css', import.meta.url))).toBe(false);
  });
});
