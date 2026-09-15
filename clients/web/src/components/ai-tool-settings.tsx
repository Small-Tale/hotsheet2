import './ai-tool-settings.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bot, Brain, Gauge, Pencil } from 'lucide';

import type { AiToolDescriptor, AiToolSelection } from './drive-options-menu';
import { Select } from './select';

export function AiToolSettings({tools,selection,loading=false,message=''}:{tools:readonly AiToolDescriptor[];selection:AiToolSelection;loading?:boolean;message?:string}){
  if(!loading&&!tools.length)return <section class="ai-tool-settings" data-component="ai-tool-settings"><div class="ai-tool-settings__empty"><LucideIcon icon={Bot} name="bot"/><strong>No AI tools detected</strong><p>Install or enable a drivable AI-tool plugin to configure Drive, AI shells, and AI chat.</p></div></section>;
  if(!tools.length)return <section class="ai-tool-settings" data-component="ai-tool-settings" aria-busy="true"><p>Loading AI tools…</p></section>;
  const active=tools.find(tool=>tool.id===selection.tool)??tools.at(0)!,modelId=(active.id===selection.tool?selection.model:undefined)??active.default_model??active.models.at(0)?.id??'',model=active.models.find(item=>item.id===modelId),customModel=modelId&&!model?modelId:undefined,efforts=model?.effort_levels??[];
  let otherModelValue='__hotsheet_other_model__';while(modelId===otherModelValue||active.models.some(item=>item.id===otherModelValue))otherModelValue+='_';
  const modelChoices=[...(customModel?[{value:customModel,label:customModel,icon:Brain,iconName:'brain'}]:[]),...active.models.map(item=>({value:item.id,label:item.label,icon:Brain,iconName:'brain'})),{value:otherModelValue,label:'Other…',icon:Pencil,iconName:'pencil',separatorBefore:true,action:'open-settings-manual-model'}];
  return <section class="ai-tool-settings" data-component="ai-tool-settings" aria-busy={String(loading)}>
    <p>Choose the machine-local defaults used by Drive, AI shells, and AI chat.</p>
    <div class="ai-tool-settings__grid">
      <Select name="ai-default-tool" label="AI provider" value={active.id} choices={tools.map(tool=>({value:tool.id,label:tool.display_name,icon:Bot,iconName:'bot'}))}/>
      <Select name="ai-default-model" label="Model" value={modelId} choices={modelChoices}/>
      <Select name="ai-default-effort" label="Effort" value={selection.effort??active.default_effort??efforts.at(0)??''} disabled={!efforts.length} choices={efforts.map(value=>({value,label:value,icon:Gauge,iconName:'gauge'}))}/>
    </div>
    <p role="status">{message}</p>
  </section>;
}
