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
  it('identifies the destination that the Drive button will use', () => {
    const markup = String(
      DriveOptionsMenu({
        tools,
        selection: { tool: 'codex' },
        defaultSelection: { tool: 'codex' },
        destination: 'Codex 2 (660A4)',
      }),
    );
    expect(markup).toContain('Destination');
    expect(markup).toContain('Codex 2 (660A4)');
  });
  it('renders plugin-discovered submenus plus a selected ephemeral custom model and Other action', () => {
    const markup = String(
      DriveOptionsMenu({
        tools,
        selection: { tool: 'codex', model: 'legacy model', effort: 'high' },
        defaultSelection: { tool: 'claude', model: 'opus' },
      }),
    );
    expect(markup).toContain('aria-label="Drive Provider, Model, and Effort Options"');
    // Kerf menu checkbox items (`role="menuitemcheckbox"`), the choices inside `data-submenu` menus.
    const submenuItem = (value: string, action: string, checked: boolean) =>
      new RegExp(
        `data-submenu=""[\\s\\S]*data-value="${value}"[^>]*role="menuitemcheckbox"[^>]*data-action="${action}" value="${value}" aria-checked="${String(checked)}"`,
      );
    expect(markup).toMatch(/role="menuitemcheckbox"[^>]*data-action="select-drive-default"/);
    expect(markup).toMatch(submenuItem('claude', 'select-drive-tool', false));
    expect(markup).toMatch(submenuItem('gpt-5.6', 'select-drive-model', false));
    expect(markup).toMatch(submenuItem('legacy model', 'select-drive-model', true));
    expect(markup).toContain('data-action="open-drive-manual-model"');
    expect(markup).not.toContain('name="drive-model"');
    // The current provider and the ephemeral custom model are checked PopupMenu choices (HS2-2EHD8R).
    expect(markup.match(/aria-checked="true"/g)).toHaveLength(2);
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
      markup.match(/role="menuitemcheckbox" tabindex="-1" data-action="select-drive-[a-z]+" value="[^"]+"/g)?.length,
    ).toBeGreaterThan(0);
    // Kerf's context PopupMenu renders its own anchor (KF-FXGW0C, HS2-H4JZDQ); the menu has no stylesheet.
    expect(markup).toContain('data-context-menu="drive-options"');
    expect(markup).not.toContain('context-menu-anchor');
    expect(existsSync(new URL('./drive-options-menu.css', import.meta.url))).toBe(false);
  });
});
