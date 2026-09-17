import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './drive-options-menu.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { RotateCcw } from 'lucide';

import { ProviderModelEffortSubmenus } from './provider-model-effort-menu';

export interface AiToolModel { id:string; label:string; effort_levels?:readonly string[] }
export interface AiToolDescriptor { id:string; display_name:string; models:readonly AiToolModel[]; default_model?:string; default_effort?:string; actions?:readonly ('change_model'|'change_effort')[] }
export interface AiToolSelection { tool?:string; model?:string; effort?:string }

export function DriveOptionsMenu({tools,selection,defaultSelection,loading=false,error}:{tools:readonly AiToolDescriptor[];selection:AiToolSelection;defaultSelection:AiToolSelection;loading?:boolean;error?:string}){
  if(!tools.length)return <div class="drive-options-menu" data-component="drive-options-menu" role="menu" aria-label="Drive provider, model, and effort options"><wa-dropdown open placement="top-start" distance={6}><span slot="trigger" class="drive-options-menu__anchor" aria-hidden="true"></span><wa-dropdown-item disabled>{loading?'Detecting AI tools…':error||'No AI tools detected'}</wa-dropdown-item></wa-dropdown></div>;
  const activeTool=tools.find(tool=>tool.id===(selection.tool??defaultSelection.tool))??tools.at(0)!,modelId=selection.model??defaultSelection.model??activeTool.default_model??activeTool.models.at(0)?.id??'',activeModel=activeTool.models.find(model=>model.id===modelId),customModel=modelId&&!activeModel?modelId:undefined,efforts=activeModel?.effort_levels??[],currentEffort=selection.effort??defaultSelection.effort??activeTool.default_effort;
  return <div class="drive-options-menu" data-component="drive-options-menu" role="menu" aria-label="Drive provider, model, and effort options">
    <wa-dropdown open placement="top-start" distance={6}>
      <span slot="trigger" class="drive-options-menu__anchor" aria-hidden="true"></span>
      <wa-dropdown-item type="checkbox" checked={!selection.tool} data-action="select-drive-default"><span slot="icon"><LucideIcon icon={RotateCcw} name="rotate-ccw"/></span>Default{defaultSelection.tool&&<span slot="details">{tools.find(tool=>tool.id===defaultSelection.tool)?.display_name??defaultSelection.tool}</span>}</wa-dropdown-item>
      <wa-divider></wa-divider>
      <ProviderModelEffortSubmenus
        actions={{provider:'select-drive-tool',model:'select-drive-model',effort:'select-drive-effort',manualModel:'open-drive-manual-model'}}
        providers={{choices:tools.map(tool=>({id:tool.id,label:tool.display_name})),currentId:activeTool.id,currentLabel:activeTool.display_name}}
        model={{choices:activeTool.models.map(model=>({id:model.id,label:model.label})),currentId:activeModel?.id,currentLabel:activeModel?.label??modelId,customModel}}
        effort={{efforts,current:currentEffort}}
      />
    </wa-dropdown>
  </div>;
}
