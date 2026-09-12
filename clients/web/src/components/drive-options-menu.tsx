import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './drive-options-menu.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bot, Brain, Gauge, type IconNode, RotateCcw } from 'lucide';

export interface AiToolModel { id:string; label:string; effort_levels?:readonly string[] }
export interface AiToolDescriptor { id:string; display_name:string; models:readonly AiToolModel[]; default_model?:string; default_effort?:string; actions?:readonly ('change_model'|'change_effort')[] }
export interface AiToolSelection { tool?:string; model?:string; effort?:string }

function choice(action:string,value:string,label:string,selected:boolean,icon:IconNode,iconName:string){return <wa-dropdown-item slot="submenu" aria-current={selected?'true':undefined} data-action={action} data-value={value} value={value}><span slot="icon"><LucideIcon icon={icon} name={iconName}/></span>{label}</wa-dropdown-item>}

export function DriveOptionsMenu({tools,selection,defaultSelection,loading=false,error}:{tools:readonly AiToolDescriptor[];selection:AiToolSelection;defaultSelection:AiToolSelection;loading?:boolean;error?:string}){
  if(!tools.length)return <div class="drive-options-menu" data-component="drive-options-menu" role="menu" aria-label="Drive provider, model, and effort options"><wa-dropdown open placement="top-start" distance={6}><span slot="trigger" class="drive-options-menu__anchor" aria-hidden="true"></span><wa-dropdown-item disabled>{loading?'Detecting AI tools…':error||'No AI tools detected'}</wa-dropdown-item></wa-dropdown></div>;
  const activeTool=tools.find(tool=>tool.id===(selection.tool??defaultSelection.tool))??tools.at(0)!,activeModel=activeTool.models.find(model=>model.id===(selection.model??defaultSelection.model??activeTool.default_model))??activeTool.models.at(0),efforts=activeModel?.effort_levels??[];
  return <div class="drive-options-menu" data-component="drive-options-menu" role="menu" aria-label="Drive provider, model, and effort options">
    <wa-dropdown open placement="top-start" distance={6}>
      <span slot="trigger" class="drive-options-menu__anchor" aria-hidden="true"></span>
      <wa-dropdown-item type="checkbox" checked={!selection.tool} data-action="select-drive-default"><span slot="icon"><LucideIcon icon={RotateCcw} name="rotate-ccw"/></span>Default{defaultSelection.tool&&<span slot="details">{tools.find(tool=>tool.id===defaultSelection.tool)?.display_name??defaultSelection.tool}</span>}</wa-dropdown-item>
      <wa-divider></wa-divider>
      <wa-dropdown-item><span slot="icon"><LucideIcon icon={Bot} name="bot"/></span>Provider<span slot="details">{activeTool.display_name}</span>{tools.map(tool=>choice('select-drive-tool',tool.id,tool.display_name,tool.id===activeTool.id,Bot,'bot'))}</wa-dropdown-item>
      <wa-dropdown-item disabled={!activeTool.models.length}><span slot="icon"><LucideIcon icon={Brain} name="brain"/></span>Model<span slot="details">{activeModel?.label}</span>{activeTool.models.map(model=>choice('select-drive-model',model.id,model.label,model.id===activeModel?.id,Brain,'brain'))}</wa-dropdown-item>
      <wa-dropdown-item disabled={!efforts.length}><span slot="icon"><LucideIcon icon={Gauge} name="gauge"/></span>Effort<span slot="details">{selection.effort??defaultSelection.effort??activeTool.default_effort}</span>{efforts.map(effort=>choice('select-drive-effort',effort,effort,effort===(selection.effort??defaultSelection.effort??activeTool.default_effort),Gauge,'gauge'))}</wa-dropdown-item>
    </wa-dropdown>
  </div>;
}
