import './ai-tool-settings.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bot, Gauge } from 'lucide';

import type { AiToolDescriptor, AiToolSelection } from './drive-options-menu';
import { ModelInput } from './model-input';
import { Select } from './select';

export function AiToolSettings({tools,selection,loading=false,message=''}:{tools:readonly AiToolDescriptor[];selection:AiToolSelection;loading?:boolean;message?:string}){
  if(!loading&&!tools.length)return <section class="ai-tool-settings" data-component="ai-tool-settings"><div class="ai-tool-settings__empty"><LucideIcon icon={Bot} name="bot"/><strong>No AI tools detected</strong><p>Install or enable a drivable AI-tool plugin to configure Drive, AI shells, and AI chat.</p></div></section>;
  if(!tools.length)return <section class="ai-tool-settings" data-component="ai-tool-settings" aria-busy="true"><p>Loading AI tools…</p></section>;
  const active=tools.find(tool=>tool.id===selection.tool)??tools.at(0)!,modelId=(active.id===selection.tool?selection.model:undefined)??active.default_model??active.models.at(0)?.id??'',model=active.models.find(item=>item.id===modelId),efforts=model?.effort_levels??[];
  return <section class="ai-tool-settings" data-component="ai-tool-settings" aria-busy={String(loading)}>
    <p>Choose the machine-local defaults used by Drive, AI shells, and AI chat.</p>
    <div class="ai-tool-settings__grid">
      <Select name="ai-default-tool" label="AI provider" value={active.id} choices={tools.map(tool=>({value:tool.id,label:tool.display_name,icon:Bot,iconName:'bot'}))}/>
      <ModelInput name="ai-default-model" label="Model" value={modelId} disabled={!modelId&&!active.models.length} choices={active.models}/>
      <Select name="ai-default-effort" label="Effort" value={selection.effort??active.default_effort??efforts.at(0)??''} disabled={!efforts.length} choices={efforts.map(value=>({value,label:value,icon:Gauge,iconName:'gauge'}))}/>
    </div>
    <p role="status">{message}</p>
  </section>;
}
