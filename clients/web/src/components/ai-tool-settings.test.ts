import {describe,expect,it} from 'vitest';

import {AiToolSettings} from './ai-tool-settings';

describe('AiToolSettings',()=>{
  it('renders plugin-provided provider, model, and compatible effort choices',()=>{const markup=String(AiToolSettings({tools:[{id:'codex',display_name:'Codex',models:[{id:'gpt',label:'GPT',effort_levels:['low','high']}],default_model:'gpt',default_effort:'high'}],selection:{tool:'codex'}}));for(const name of ['ai-default-tool','ai-default-model','ai-default-effort'])expect(markup).toContain(`name="${name}"`);expect(markup).toContain('value="high"');expect(markup).toContain('machine-local defaults')});
  it('explains the empty detected-plugin state',()=>{expect(String(AiToolSettings({tools:[],selection:{}}))).toContain('No AI tools detected')});
});
