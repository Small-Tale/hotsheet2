import { describe,expect,it } from 'vitest';

import { DriveOptionsMenu } from './drive-options-menu';

const tools=[{id:'codex',display_name:'Codex',default_model:'gpt-5.6',default_effort:'high',models:[{id:'gpt-5.6',label:'GPT-5.6',effort_levels:['medium','high']}]},{id:'claude',display_name:'Claude',models:[{id:'opus',label:'Opus'}]}];

describe('DriveOptionsMenu',()=>{
  it('renders plugin-discovered provider, model, and effort submenus with a default reset',()=>{const markup=String(DriveOptionsMenu({tools,selection:{tool:'codex',model:'gpt-5.6',effort:'high'},defaultSelection:{tool:'claude',model:'opus'}}));expect(markup).toContain('aria-label="Drive provider, model, and effort options"');expect(markup).toContain('data-action="select-drive-default"');expect(markup).toContain('data-action="select-drive-tool" data-value="claude"');expect(markup).toContain('data-action="select-drive-model" data-value="gpt-5.6"');expect(markup).toContain('data-action="select-drive-effort" data-value="high"');expect(markup).toContain('checked');});
  it('disables model effort choices when a plugin exposes none',()=>{const markup=String(DriveOptionsMenu({tools:[{id:'plain',display_name:'Plain',models:[]}],selection:{tool:'plain'},defaultSelection:{tool:'plain'}}));expect(markup.match(/disabled/g)?.length).toBeGreaterThanOrEqual(2)});
});
