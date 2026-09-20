import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import {COMMAND_EDITOR_DIALOG_ID,CommandSettingsEditor} from './command-settings-editor';

const css=readFileSync(new URL('./command-settings-editor.css',import.meta.url),'utf8');

describe('CommandSettingsEditor',()=>{
  it('uses the kerf semantic spacing scale for app-owned separation (HS2-4Y6SM9)',()=>{
    expect(css).not.toContain('--wa-space-');
    expect(css).toContain('gap:var(--kui-space-l)');
    expect(css).toContain('gap:var(--kui-space-none)');
  });
  it('renders draggable grouped rows with an overflow menu and no arrows or Save button (HS2-D9JBXT)',()=>{
    const markup=String(CommandSettingsEditor({commands:[
      {id:'a',title:'Run tests',kind:'shell',command:'npm test',group:'Quality'},
      {id:'b',title:'Build',kind:'program',program:'npm',args:['run','build'],group:'Quality'},
    ]}));
    expect(markup).toContain('data-component="command-settings-editor"');
    // Rows are draggable and carry their id; the group header renders once.
    expect(markup).toMatch(/class="command-settings-editor__row" data-command-id="a"[^>]*draggable="true"/);
    expect(markup).toMatch(/command-settings-editor__group-label">Quality</);
    // Actions live in a per-row overflow menu (edit/delete), not inline arrow/delete icons.
    expect(markup).toContain('command-settings-editor__row-menu');
    expect(markup).toContain('data-action="edit-command-setting"');
    expect(markup).toContain('data-action="delete-command-setting"');
    expect(markup).not.toContain('data-action="move-command-setting"');
    // Header offers Add group + Add command; the list autosaves (no explicit Save button).
    expect(markup).toContain('data-action="add-command-group"');
    expect(markup).toContain('data-action="add-command-setting"');
    expect(markup).not.toContain('data-action="save-command-settings"');
    expect(markup).toContain('save automatically');
  });

  it('marks the multi-selected rows as selected for drag-together reordering (HS2-VJYQHG)',()=>{
    const markup=String(CommandSettingsEditor({commands:[
      {id:'a',title:'A',kind:'shell',command:'x'},
      {id:'b',title:'B',kind:'shell',command:'y'},
      {id:'c',title:'C',kind:'shell',command:'z'},
    ],selectedIds:['a','c']}));
    expect(markup).toMatch(/data-command-id="a"[^>]*data-selected="true"[^>]*aria-selected="true"/);
    expect(markup).toMatch(/data-command-id="c"[^>]*data-selected="true"/);
    expect(markup).not.toMatch(/data-command-id="b"[^>]*data-selected="true"/);
    expect(markup).toContain('aria-multiselectable="true"');
  });

  it('renders an empty added group with a droppable area and a delete button (HS2-D9JBXT)',()=>{
    const markup=String(CommandSettingsEditor({commands:[{id:'a',title:'A',kind:'shell',command:'x'}],extraGroups:['Ideas']}));
    expect(markup).toContain('command-settings-editor__group-label">Ideas<');
    expect(markup).toMatch(/data-action="delete-command-group" data-group="Ideas"/);
    expect(markup).toContain('Drag commands here');
    // Ungrouped command 'a' has a drop container keyed to the blank group.
    expect(markup).toContain('data-command-group-drop=""');
  });

  it('opens a details dialog only for the edited command, marking its row',()=>{
    const props={commands:[{id:'c',title:'Verify',kind:'shell' as const,command:'npm test',color:'#22c55e',icon:'test'}]};
    const closed=String(CommandSettingsEditor(props));
    expect(closed).toContain(`id="${COMMAND_EDITOR_DIALOG_ID}"`);
    expect(closed).not.toContain('name="command"');
    expect(closed).not.toContain('data-editing="true"');

    const open=String(CommandSettingsEditor({...props,editingId:'c'}));
    expect(open).toContain('data-editing="true"');
    expect(open).toContain('Edit command');
    expect(open).toContain('data-action="close-command-editor"');
    expect(open).toMatch(/data-command-id="c"[^]*name="command"/);
  });

  it('offers color swatches and a searchable Lucide icon picker reflecting the edited command values (HS2-5VSNV3)',()=>{
    const markup=String(CommandSettingsEditor({commands:[{id:'c',title:'Verify',kind:'shell',command:'npm test',color:'#22c55e',icon:'test'}],editingId:'c'}));
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

  it('drops the identifier/group/working-directory fields and offers a Transparent color (HS2-ZQWQCM)',()=>{
    const markup=String(CommandSettingsEditor({commands:[{id:'c',title:'Verify',kind:'shell',command:'npm test'}],editingId:'c'}));
    expect(markup).not.toContain('name="id"');
    expect(markup).not.toContain('name="group"');
    expect(markup).not.toContain('name="cwd"');
    expect(markup).toMatch(/name="color"[^>]*value="transparent"/);
    expect(markup).toContain('Transparent');
    // An uncolored command selects Transparent by default.
    expect(markup).toMatch(/name="color"[^>]*value="transparent"[^>]*checked/);
  });
  it('shows type-specific shell and AI fields in the dialog',()=>{
    expect(String(CommandSettingsEditor({commands:[{id:'shell',title:'Shell',kind:'shell',command:'npm test'}],editingId:'shell'}))).toContain('name="command"');
    const tools=[{id:'codex',display_name:'Codex',models:[{id:'gpt-6',label:'GPT-6',effort_levels:['low','high']}],default_model:'gpt-6',default_effort:'low',actions:[]},{id:'claude',display_name:'Claude',models:[{id:'sonnet',label:'Sonnet',effort_levels:['medium','high']}],default_model:'sonnet',default_effort:'medium',actions:[]}];
    const inherited=String(CommandSettingsEditor({commands:[{id:'review',title:'Review',kind:'ai',prompt:'Review this'}],editingId:'review',aiTools:tools,aiDefaults:{tool:'codex',model:'gpt-6',effort:'high'}}));
    expect(inherited).toContain('AI configuration: Project Default');
    expect(inherited).toContain('data-action="select-command-ai-default"');
    const ai=String(CommandSettingsEditor({commands:[{id:'review',title:'Review',kind:'ai',prompt:'Review this',tool:'claude',model:'sonnet',effort:'high'}],editingId:'review',aiTools:tools,aiDefaults:{tool:'codex'}}));
    expect(ai).toContain('name="prompt"');
    expect(ai).toContain('AI configuration: Claude · Sonnet · high');
    expect(ai).toContain('data-action="select-command-ai-tool"');
    expect(ai).toContain('data-action="select-command-ai-model"');
    expect(ai).toContain('data-action="select-command-ai-effort"');
    expect(ai).toContain('data-action="open-command-manual-model"');
    expect(ai).not.toContain('name="tool"');
    expect(ai).not.toContain('name="program"');
  });

  it('only lays out the popover dialog when it is open, so the closed dialog stays UA-hidden (HS2-D9JBXT)',()=>{
    // Setting `display` unconditionally on a `popover` element overrides [popover]:not(:popover-open){display:none},
    // rendering the empty closed dialog as a thin bordered strip — the stray line the maintainer reported.
    expect(css).not.toMatch(/\.command-settings-editor__dialog\s*\{[^}]*display\s*:/);
    expect(css).toMatch(/\.command-settings-editor__dialog:popover-open\s*\{[^}]*display\s*:\s*flex/);
  });
  it('provides a useful empty state with Add command and Add group affordances',()=>{
    const markup=String(CommandSettingsEditor({commands:[]}));
    expect(markup).toContain('No custom commands yet');
    expect(markup).toContain('data-action="add-command-setting"');
    expect(markup).toContain('data-action="add-command-group"');
    expect(markup).not.toContain('data-action="save-command-settings"');
  });
});
