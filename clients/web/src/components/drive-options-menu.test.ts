import { readFileSync } from 'node:fs';

import { describe,expect,it } from 'vitest';

import { DriveOptionsMenu } from './drive-options-menu';

const tools=[{id:'codex',display_name:'Codex',default_model:'gpt-5.6',default_effort:'high',models:[{id:'gpt-5.6',label:'GPT-5.6',effort_levels:['medium','high']}]},{id:'claude',display_name:'Claude',models:[{id:'opus',label:'Opus'}]}];

describe('DriveOptionsMenu',()=>{
  it('renders plugin-discovered provider, model, and effort submenus with a default reset',()=>{const markup=String(DriveOptionsMenu({tools,selection:{tool:'codex',model:'gpt-5.6',effort:'high'},defaultSelection:{tool:'claude',model:'opus'}}));expect(markup).toContain('aria-label="Drive provider, model, and effort options"');expect(markup).toContain('type="checkbox" data-action="select-drive-default"');expect(markup).toContain('data-action="select-drive-tool" data-value="claude"');expect(markup).toContain('data-action="select-drive-model" data-value="gpt-5.6"');expect(markup).toContain('data-action="select-drive-effort" data-value="high"');expect(markup.match(/aria-current="true"/g)).toHaveLength(3);});
  it('disables model effort choices when a plugin exposes none',()=>{const markup=String(DriveOptionsMenu({tools:[{id:'plain',display_name:'Plain',models:[]}],selection:{tool:'plain'},defaultSelection:{tool:'plain'}}));expect(markup.match(/disabled/g)?.length).toBeGreaterThanOrEqual(2)});
  it('distinguishes active discovery and a discovery error from a confirmed empty catalog',()=>{const loading=String(DriveOptionsMenu({tools:[],selection:{},defaultSelection:{},loading:true})),failed=String(DriveOptionsMenu({tools:[],selection:{},defaultSelection:{},error:'AI discovery unavailable'}));expect(loading).toContain('Detecting AI tools…');expect(loading).not.toContain('No AI tools detected');expect(failed).toContain('AI discovery unavailable');expect(failed).not.toContain('No AI tools detected')});
  it('uses the shared icon-label selection hierarchy without a duplicate disclosure icon',()=>{const markup=String(DriveOptionsMenu({tools,selection:{tool:'codex',model:'gpt-5.6',effort:'high'},defaultSelection:{tool:'claude',model:'opus'}}));expect(markup).not.toContain('data-lucide="chevron-right"');expect(markup.match(/data-lucide="bot"/g)).toHaveLength(3);expect(markup.match(/data-lucide="brain"/g)).toHaveLength(2);expect(markup.match(/data-lucide="gauge"/g)).toHaveLength(3);expect(markup.match(/slot="submenu" type="checkbox"/g)).toBeNull();const css=readFileSync(new URL('./drive-options-menu.css',import.meta.url),'utf8');expect(css).toContain('wa-dropdown-item[slot="submenu"] { min-width:13rem; padding-inline:.75rem; }');expect(css).toContain('wa-dropdown-item[slot="submenu"][aria-current="true"]');});
});
