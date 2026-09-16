import {describe,expect,it} from 'vitest';

import {COMMAND_EDITOR_DIALOG_ID,CommandSettingsEditor} from './command-settings-editor';

describe('CommandSettingsEditor',()=>{
  it('renders commands as a grouped WYSIWYG list with inline edit/reorder/delete actions',()=>{
    const markup=String(CommandSettingsEditor({commands:[
      {id:'a',title:'Run tests',kind:'shell',command:'npm test',group:'Quality'},
      {id:'b',title:'Build',kind:'program',program:'npm',args:['run','build'],group:'Quality'},
    ]}));
    expect(markup).toContain('data-component="command-settings-editor"');
    expect(markup).toContain('class="command-settings-editor__list"');
    // The group header is rendered once for the shared group.
    expect(markup).toMatch(/command-settings-editor__group-label">Quality</);
    // Each row carries its command id plus edit and delete affordances.
    expect(markup).toContain('data-command-id="a"');
    expect(markup).toContain('data-action="edit-command-setting"');
    expect(markup).toContain('data-action="delete-command-setting"');
    expect(markup).toContain('data-action="move-command-setting"');
    expect(markup).toContain('Save commands');
    // Reorder is bounded: the first row cannot move up, the last cannot move down.
    expect(markup).toMatch(/data-direction="up" aria-label="Move Run tests up" disabled/);
    expect(markup).toMatch(/data-direction="down" aria-label="Move Build down" disabled/);
  });

  it('opens a details dialog only for the edited command, marking its row',()=>{
    const props={commands:[{id:'c',title:'Verify',kind:'shell' as const,command:'npm test',color:'#22c55e',icon:'test'}]};
    const closed=String(CommandSettingsEditor(props));
    expect(closed).toContain(`id="${COMMAND_EDITOR_DIALOG_ID}"`);
    expect(closed).not.toContain('name="command"'); // No detail fields until a command is being edited.
    expect(closed).not.toContain('data-editing="true"');

    const open=String(CommandSettingsEditor({...props,editingId:'c'}));
    expect(open).toContain('data-editing="true"');
    expect(open).toContain('Edit command');
    expect(open).toContain('data-action="close-command-editor"');
    // The dialog hosts the detail form for the edited command.
    expect(open).toMatch(/data-command-id="c"[^]*name="command"/);
  });

  it('offers color swatches and an icon picker reflecting the edited command values',()=>{
    const markup=String(CommandSettingsEditor({commands:[{id:'c',title:'Verify',kind:'shell',command:'npm test',color:'#22c55e',icon:'test'}],editingId:'c'}));
    expect(markup).toContain('name="color"');
    expect(markup).toContain('--swatch:#22c55e');
    expect(markup).toMatch(/name="color"[^>]*value="#22c55e"[^>]*checked/);
    expect(markup).toContain('name="icon"');
    expect(markup).toMatch(/name="icon"[^>]*value="test"[^>]*checked/);
    expect(markup).toContain('value="build"');
  });

  it('shows type-specific shell and AI fields in the dialog',()=>{
    expect(String(CommandSettingsEditor({commands:[{id:'shell',title:'Shell',kind:'shell',command:'npm test'}],editingId:'shell'}))).toContain('name="command"');
    const ai=String(CommandSettingsEditor({commands:[{id:'review',title:'Review',kind:'ai',prompt:'Review this',tool:'claude'}],editingId:'review'}));
    expect(ai).toContain('name="prompt"');
    expect(ai).toContain('name="tool"');
    expect(ai).not.toContain('name="program"');
  });

  it('provides a useful empty state and still allows saving an empty command list',()=>{
    const markup=String(CommandSettingsEditor({commands:[]}));
    expect(markup).toContain('No custom commands yet');
    expect(markup).toContain('Add a command');
    expect(markup).toContain('data-action="save-command-settings"');
    expect(markup).not.toMatch(/data-action="save-command-settings" disabled/);
  });
});
