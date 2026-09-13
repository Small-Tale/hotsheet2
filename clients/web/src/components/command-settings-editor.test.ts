import {describe,expect,it} from 'vitest';

import {CommandSettingsEditor} from './command-settings-editor';

describe('CommandSettingsEditor',()=>{
  it('renders a selected program command as an accessible master-detail editor',()=>{const markup=String(CommandSettingsEditor({commands:[{id:'test',title:'Run tests',program:'npm',args:['run','test'],group:'Quality'}],selectedId:'test'}));expect(markup).toContain('data-component="command-settings-editor"');expect(markup).toContain('aria-current="page"');expect(markup).toContain('name="program"');expect(markup).toContain('run\ntest');expect(markup).toContain('Save commands')});
  it('shows type-specific shell and AI fields',()=>{expect(String(CommandSettingsEditor({commands:[{id:'shell',title:'Shell',kind:'shell',command:'npm test'}]}))).toContain('name="command"');const ai=String(CommandSettingsEditor({commands:[{id:'review',title:'Review',kind:'ai',prompt:'Review this',tool:'claude'}]}));expect(ai).toContain('name="prompt"');expect(ai).toContain('name="tool"');expect(ai).not.toContain('name="program"')});
  it('provides a useful empty state and still allows saving an empty command list',()=>{const markup=String(CommandSettingsEditor({commands:[]}));expect(markup).toContain('No custom commands yet');expect(markup).toContain('Add a command');expect(markup).toContain('data-action="save-command-settings"');expect(markup).not.toMatch(/data-action="save-command-settings" disabled/)});
});
