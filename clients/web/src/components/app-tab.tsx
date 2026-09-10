import './app-tab.css';

import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { X } from 'lucide';

import { LucideIcon } from './lucide-icon';

export interface AppTabProps {
  kind:'project'|'terminal';
  id:string;
  name:string;
  selected?:boolean;
  closable?:boolean;
  leading?:SafeHtml;
  trailing?:SafeHtml;
  rootAttributes?:Record<string,string>;
}

export function AppTab({kind,id,name,selected=false,closable=true,leading,trailing,rootAttributes={}}:AppTabProps){
  const idAttribute=kind==='project'?{'data-project-id':id}:{'data-terminal-id':id},selectAction=kind==='project'?'select-project-tab':'select-drawer-terminal',closeAction=kind==='project'?'close-project-tab':'close-terminal-tab';
  return <div class={`app-tab ${kind}-tab`} data-component={`${kind}-tab`} data-tab-kind={kind} data-tab-id={id} data-selected={String(selected)} draggable="true" {...idAttribute} {...rootAttributes}>
    {closable&&<button type="button" tabindex="-1" class={`app-tab__close ${kind}-tab__close`} data-action={closeAction} {...idAttribute} aria-label={`Close ${name}`} title={`Close ${name}`}><LucideIcon icon={X} name="x"/></button>}
    <button type="button" class={`app-tab__select ${kind}-tab__select`} role="tab" aria-selected={String(selected)} aria-keyshortcuts="Delete Backspace" data-action={selectAction} {...idAttribute} tabindex="0">
      {leading}<span class={`app-tab__name ${kind}-tab__name`}>{name}</span>{closable||trailing?<span class={`app-tab__trailing ${kind}-tab__trailing`}>{trailing}</span>:undefined}
    </button>
  </div>;
}
