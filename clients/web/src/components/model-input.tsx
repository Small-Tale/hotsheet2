import './model-input.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ChevronsUpDown } from 'lucide';

export interface ModelInputChoice { id:string; label:string }

export function ModelInput({name,label,value,choices,disabled=false,className='',listId=`${name}-choices`}:{name:string;label:string;value:string;choices:readonly ModelInputChoice[];disabled?:boolean;className?:string;listId?:string}){
  return <label class={`model-input ${className}`.trim()}><span class="model-input__label">{label}</span><span class="model-input__control"><input type="text" name={name} value={value} list={listId} autocomplete="off" spellcheck="false" required disabled={disabled}/><LucideIcon icon={ChevronsUpDown} name="chevrons-up-down"/></span><datalist id={listId}>{choices.map(choice=><option value={choice.id}>{choice.label}</option>)}</datalist></label>;
}
