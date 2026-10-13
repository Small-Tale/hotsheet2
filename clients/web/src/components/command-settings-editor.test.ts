import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { COMMAND_EDITOR_DIALOG_ID, CommandSettingsEditor } from './command-settings-editor';

const css = readFileSync(new URL('./command-settings-editor.css', import.meta.url), 'utf8');

describe('CommandSettingsEditor', () => {
  it('puts the description, group headers, and row grips on one text edge (HS2-1711Z5)', () => {
    // Web Awesome's native li indent is reset, and rows pad like Kerf Text item geometry.
    expect(css).toMatch(
      /\.command-settings-editor__section,\s*\.command-settings-editor__row,\s*\.command-settings-editor__group-empty \{\s*margin: 0;/,
    );
    expect(css).toMatch(
      /\.command-settings-editor__row \{[^}]*padding: var\(--kui-space-2xs\) var\(--kui-space-xs\);[^}]*border: 1px solid transparent;/,
    );
    const markup = String(
      CommandSettingsEditor({
        commands: [{ id: 'a', title: 'Lint', group: 'Quality', kind: 'shell', command: 'npm run lint' }],
      }),
    );
    expect(markup.match(/<p[^>]*>\s*Create the buttons/)?.[0]).not.toContain('data-flush');
    expect(markup).not.toContain('data-text-insets');
  });
  it('uses the kerf semantic spacing scale for app-owned separation (HS2-4Y6SM9)', () => {
    expect(css).not.toContain('--wa-space-');
    expect(css).toContainSource('gap:var(--kui-space-l)');
    expect(css).toContainSource('gap:var(--kui-space-none)');
  });
  it('renders draggable grouped rows with an overflow menu and no arrows or Save button (HS2-D9JBXT)', () => {
    const markup = String(
      CommandSettingsEditor({
        commands: [
          { id: 'a', title: 'Run tests', kind: 'shell', command: 'npm test', group: 'Quality' },
          { id: 'b', title: 'Build', kind: 'program', program: 'npm', args: ['run', 'build'], group: 'Quality' },
        ],
      }),
    );
    expect(markup).toContain('data-component="command-settings-editor"');
    // Rows are draggable and carry their id; the group header renders once.
    expect(markup).toMatch(/class="command-settings-editor__row" data-command-id="a"[^>]*draggable="true"/);
    expect(markup).toMatch(/data-component="list-header"[^]*Quality/);
    // Actions live in a per-row overflow menu (edit/delete), not inline arrow/delete icons.
    expect(markup).toContain('command-settings-editor__row-menu');
    expect(markup).toContain('data-action="edit-command-setting"');
    expect(markup).toContain('data-action="delete-command-setting"');
    expect(markup).not.toContain('data-action="move-command-setting"');
    // Add group… / Add command… live in the workspace toolbar (HS2-YKB7HS); the list autosaves
    // (no explicit Save button).
    expect(markup).not.toContain('data-action="add-command-group"');
    expect(markup).not.toContain('data-action="add-command-setting"');
    expect(markup).not.toContain('data-action="save-command-settings"');
    expect(markup).toContain('save automatically');
    // The workspace toolbar titles the page, so no in-page "Custom commands" heading repeats it
    // (HS2-QEQ1YB).
    expect(markup).not.toContain('aria-level="2"');
    expect(markup).not.toContain('command-settings-editor__heading');
    expect(css).not.toContain('__heading');
    expect(css).not.toContain('@container');
  });

  it('marks the multi-selected rows as selected for drag-together reordering (HS2-VJYQHG)', () => {
    const markup = String(
      CommandSettingsEditor({
        commands: [
          { id: 'a', title: 'A', kind: 'shell', command: 'x' },
          { id: 'b', title: 'B', kind: 'shell', command: 'y' },
          { id: 'c', title: 'C', kind: 'shell', command: 'z' },
        ],
        selectedIds: ['a', 'c'],
      }),
    );
    expect(markup).toMatch(/data-command-id="a"[^>]*data-selected="true"[^>]*aria-selected="true"/);
    expect(markup).toMatch(/data-command-id="c"[^>]*data-selected="true"/);
    expect(markup).not.toMatch(/data-command-id="b"[^>]*data-selected="true"/);
    expect(markup).toContain('aria-multiselectable="true"');
  });

  it('renders an empty added group with a droppable area and a delete button (HS2-D9JBXT)', () => {
    const markup = String(
      CommandSettingsEditor({
        commands: [{ id: 'a', title: 'A', kind: 'shell', command: 'x' }],
        extraGroups: ['Ideas'],
      }),
    );
    // The empty group's header is a Kerf ListHeader whose logical-end action deletes it (HS2-7BBRJT).
    expect(markup).toMatch(/data-component="list-header"[^]*Ideas/);
    expect(markup).toMatch(/<button[^>]*data-action="delete-command-group"[^>]*>/);
    expect(markup).toMatch(/data-group="Ideas"/);
    expect(markup).toContain('aria-label="Delete empty group Ideas"');
    expect(markup).toContain('Drag commands here');
    // Ungrouped command 'a' has a drop container keyed to the blank group.
    expect(markup).toContain('data-command-group-drop=""');
  });

  it('opens a details dialog only for the edited command, marking its row', () => {
    const props = {
      commands: [
        { id: 'c', title: 'Verify', kind: 'shell' as const, command: 'npm test', color: '#22c55e', icon: 'test' },
      ],
    };
    const closed = String(CommandSettingsEditor(props));
    expect(closed).toContain(`id="${COMMAND_EDITOR_DIALOG_ID}"`);
    expect(closed).not.toContain('name="command"');
    expect(closed).not.toContain('data-editing="true"');

    const open = String(CommandSettingsEditor({ ...props, editingId: 'c' }));
    expect(open).toContain('data-editing="true"');
    expect(open).toContain('Edit command');
    expect(open).toMatch(/data-appearance="borderless"[^>]*><span class="command-settings-editor__dialog-icon"/);
    expect(css).toMatch(/\.command-settings-editor__dialog-icon\s*\{[^}]*margin-inline:\s*auto;/);
    expect(open).toContain('data-action="close-command-editor"');
    expect(open).toMatch(/data-command-id="c"[^]*name="command"/);
  });

  it('offers color swatches and a searchable Lucide icon picker reflecting the edited command values (HS2-5VSNV3)', () => {
    const markup = String(
      CommandSettingsEditor({
        commands: [{ id: 'c', title: 'Verify', kind: 'shell', command: 'npm test', color: '#22c55e', icon: 'test' }],
        editingId: 'c',
      }),
    );
    expect(markup).toContain('name="color"');
    expect(markup).toContain('--swatch:#22c55e');
    expect(markup).toMatch(/name="color"[^>]*value="#22c55e"[^>]*checked/);
    // The icon field is the reusable searchable picker, not a fixed radio grid.
    expect(markup).toContain('data-component="lucide-icon-picker"');
    expect(markup).toContain('name="command-icon-search"');
    expect(markup).toContain('data-action="select-command-icon"');
    // Legacy alias `test` resolves to the Lucide id `test-tube-2` and shows as selected.
    expect(markup).toMatch(/data-icon-name="test-tube-2" aria-pressed="true"/);
    expect(markup).not.toContain('name="icon"');
  });

  it('drops the identifier/group/working-directory fields and offers a Transparent color (HS2-ZQWQCM)', () => {
    const markup = String(
      CommandSettingsEditor({
        commands: [{ id: 'c', title: 'Verify', kind: 'shell', command: 'npm test' }],
        editingId: 'c',
      }),
    );
    expect(markup).not.toContain('name="id"');
    expect(markup).not.toContain('name="group"');
    expect(markup).not.toContain('name="cwd"');
    expect(markup).toMatch(/name="color"[^>]*value="transparent"/);
    expect(markup).toContain('Transparent');
    // An uncolored command selects Transparent by default.
    expect(markup).toMatch(/name="color"[^>]*value="transparent"[^>]*checked/);
  });
  it('shows type-specific shell and AI fields in the dialog', () => {
    expect(
      String(
        CommandSettingsEditor({
          commands: [{ id: 'shell', title: 'Shell', kind: 'shell', command: 'npm test' }],
          editingId: 'shell',
        }),
      ),
    ).toContain('name="command"');
    const tools = [
      {
        id: 'codex',
        display_name: 'Codex',
        models: [{ id: 'gpt-6', label: 'GPT-6', effort_levels: ['low', 'high'] }],
        default_model: 'gpt-6',
        default_effort: 'low',
        actions: [],
      },
      {
        id: 'claude',
        display_name: 'Claude',
        models: [{ id: 'sonnet', label: 'Sonnet', effort_levels: ['medium', 'high'] }],
        default_model: 'sonnet',
        default_effort: 'medium',
        actions: [],
      },
    ];
    const inherited = String(
      CommandSettingsEditor({
        commands: [{ id: 'review', title: 'Review', kind: 'ai', prompt: 'Review this' }],
        editingId: 'review',
        aiTools: tools,
        aiDefaults: { tool: 'codex', model: 'gpt-6', effort: 'high' },
      }),
    );
    expect(inherited).toContain('<span>Project Default</span>');
    expect(inherited).toContain('data-action="select-command-ai-default"');
    const ai = String(
      CommandSettingsEditor({
        commands: [
          {
            id: 'review',
            title: 'Review',
            kind: 'ai',
            prompt: 'Review this',
            tool: 'claude',
            model: 'sonnet',
            effort: 'high',
          },
        ],
        editingId: 'review',
        aiTools: tools,
        aiDefaults: { tool: 'codex' },
      }),
    );
    expect(ai).toContain('name="prompt"');
    expect(ai).toContain('<span>Claude · Sonnet · high</span>');
    expect(ai).toContain('data-action="select-command-ai-tool"');
    expect(ai).toContain('data-action="select-command-ai-model"');
    expect(ai).toContain('data-action="select-command-ai-effort"');
    expect(ai).toContain('data-action="open-command-manual-model"');
    expect(ai).not.toContain('name="tool"');
    expect(ai).not.toContain('name="program"');
  });

  it('composes the edit fields from Web Awesome inputs and Kerf Select, Grid, and List (HS2-WNXRG3)', () => {
    const program = String(
      CommandSettingsEditor({
        commands: [{ id: 'p', title: 'Build', kind: 'program', program: 'npm', args: ['run', 'build'] }],
        editingId: 'p',
      }),
    );
    expect(program).toMatch(/<wa-input label="Button label" name="title" data-command-field[^>]*value="Build"/);
    expect(program).toMatch(/<wa-input label="Program" name="program"/);
    expect(program).toMatch(/<wa-textarea label="Arguments" name="args"[^>]*hint="One exact argument per line\."/);
    expect(program).toContain('wa-input label="Confirmation message" name="confirmation"');
    expect(program).toMatch(/<wa-select[^>]*name="kind"/);
    expect(program).toContain('data-component="grid"');
    // Every command field is a Web Awesome control; only the color swatches remain native radios.
    const nativeFields = [...program.matchAll(/<(input|textarea|select)[\s>][^>]*/g)]
      .map((match) => match[0])
      .filter((field) => !field.includes('type="radio"'));
    expect(nativeFields).toEqual([]);
    for (const removed of ['__control', '__field', '__hint', '__grid', '__wide'])
      expect(css).not.toContain(`.command-settings-editor${removed}`);
    expect(css).not.toContain('@media (max-width');
    const shell = String(
      CommandSettingsEditor({
        commands: [{ id: 's', title: 'Test', kind: 'shell', command: 'npm test' }],
        editingId: 's',
      }),
    );
    expect(shell).toMatch(/<wa-textarea label="Shell command" name="command"[^>]*hint="Runs in the project root/);
  });

  it('only lays out the popover dialog when it is open, so the closed dialog stays UA-hidden (HS2-D9JBXT)', () => {
    // Setting `display` unconditionally on a `popover` element overrides [popover]:not(:popover-open){display:none},
    // rendering the empty closed dialog as a thin bordered strip — the stray line the maintainer reported.
    expect(css).not.toMatch(/\.command-settings-editor__dialog\s*\{[^}]*display\s*:/);
    expect(css).toMatch(/\.command-settings-editor__dialog:popover-open\s*\{[^}]*display\s*:\s*flex/);
  });
  it('provides a useful empty state pointing at the toolbar Add command… action', () => {
    const markup = String(CommandSettingsEditor({ commands: [] }));
    expect(markup).toContain('No custom commands yet');
    expect(markup).not.toContain('data-action="save-command-settings"');
  });
});
