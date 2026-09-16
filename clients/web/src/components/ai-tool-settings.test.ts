import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import {AiToolSettings} from './ai-tool-settings';

describe('AiToolSettings',()=>{
  it('renders plugin models, an ephemeral selected custom model, Other, and compatible efforts',()=>{const markup=String(AiToolSettings({tools:[{id:'codex',display_name:'Codex',models:[{id:'gpt',label:'GPT',effort_levels:['low','high']}],default_model:'gpt',default_effort:'high'}],selection:{tool:'codex',model:'legacy model'}}));for(const name of ['ai-default-tool','ai-default-model','ai-default-effort'])expect(markup).toContain(`name="${name}"`);expect(markup).toContain('<wa-option value="legacy model"');expect(markup).toContain('<wa-option value="gpt"');expect(markup).toContain('data-other-model-value="__hotsheet_other_model__"');expect(markup).toContain('Other…');expect(markup).toContain('machine-local defaults')});
  it('explains the empty detected-plugin state',()=>{expect(String(AiToolSettings({tools:[],selection:{}}))).toContain('No AI tools detected')});
  it('does not carry a stale model across a replaced provider catalog',()=>{const markup=String(AiToolSettings({tools:[{id:'new',display_name:'New',models:[{id:'new-default',label:'New default'}],default_model:'new-default'}],selection:{tool:'old',model:'old-model'}}));expect(markup).toContain('name="ai-default-model" label="Model" value="new-default"');expect(markup).not.toContain('value="old-model"')});
  it('responds to the settings pane width and gives model ids the widest column',()=>{const css=readFileSync(new URL('./ai-tool-settings.css',import.meta.url),'utf8');expect(css).toContain('container-type:inline-size');expect(css).toContain('minmax(remify(224px),1.6fr)');expect(css).toContain('@container (max-width:remify(640px))')});
});
