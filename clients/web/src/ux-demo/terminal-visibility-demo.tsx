import { signal } from 'kerfjs';
import { Eye } from 'lucide';

import { LucideIcon } from '../components/lucide-icon';
import { TerminalVisibilityDialog,TerminalVisibilityNameDialog,type TerminalVisibilityNamePrompt } from '../components/terminal-visibility-dialog';
import { activeTerminalVisibilityGroup,addTerminalVisibilityGroup,initialTerminalVisibilityState,removeTerminalVisibilityGroup,renameTerminalVisibilityGroup,selectTerminalVisibilityGroup,setAllTerminalsVisibleInGroup,setTerminalVisibleInGroup } from '../terminal-visibility';

export const terminalVisibilityDemoGroups=[{projectId:'demo',projectName:'Demo project',sessions:[{id:'ai',projectId:'demo',projectName:'Demo project',title:'AI',alive:true,busy:true,scrollback:''},{id:'server',projectId:'demo',projectName:'Demo project',title:'Development server',alive:true,busy:false,scrollback:''}]}];
const seeded=addTerminalVisibilityGroup(initialTerminalVisibilityState(),'focus','Focus');
export const terminalVisibilityDemoState=signal(setTerminalVisibleInGroup(selectTerminalVisibilityGroup(seeded.state,'dashboard','focus'),'focus','demo:server',false));
export const terminalVisibilityDemoOpen=signal(false),terminalVisibilityDemoContextMenu=signal<{id:string;x:number;y:number}|undefined>(undefined),terminalVisibilityDemoNamePrompt=signal<TerminalVisibilityNamePrompt|undefined>(undefined);
export function TerminalVisibilityDialogDemo(){return <section class="terminal-visibility-demo"><wa-button appearance="accent" data-action="show-terminal-visibility-demo" aria-haspopup="dialog"><LucideIcon icon={Eye} name="eye"/>Show / Hide Terminals</wa-button><TerminalVisibilityDialog open={terminalVisibilityDemoOpen.value} state={terminalVisibilityDemoState.value} scope="dashboard" groups={terminalVisibilityDemoGroups} contextMenu={terminalVisibilityDemoContextMenu.value}/><TerminalVisibilityNameDialog prompt={terminalVisibilityDemoNamePrompt.value}/></section>}
export function showTerminalVisibilityDemo(){terminalVisibilityDemoOpen.value=true}
export function closeTerminalVisibilityDemo(){terminalVisibilityDemoOpen.value=false;terminalVisibilityDemoContextMenu.value=undefined;terminalVisibilityDemoNamePrompt.value=undefined}
const active=()=>activeTerminalVisibilityGroup(terminalVisibilityDemoState.value,'dashboard');
export function selectTerminalVisibilityDemoGroup(id:string){terminalVisibilityDemoState.value=selectTerminalVisibilityGroup(terminalVisibilityDemoState.value,'dashboard',id)}
export function promptAddTerminalVisibilityDemoGroup(){terminalVisibilityDemoContextMenu.value=undefined;terminalVisibilityDemoNamePrompt.value={mode:'add',value:''}}
export function submitTerminalVisibilityDemoName(name:string){const prompt=terminalVisibilityDemoNamePrompt.value;if(!prompt||!name.trim())return;if(prompt.mode==='add'){const added=addTerminalVisibilityGroup(terminalVisibilityDemoState.value,`demo-${terminalVisibilityDemoState.value.groups.length}`,name);terminalVisibilityDemoState.value=selectTerminalVisibilityGroup(added.state,'dashboard',added.group.id)}else if(prompt.groupId)terminalVisibilityDemoState.value=renameTerminalVisibilityGroup(terminalVisibilityDemoState.value,prompt.groupId,name);terminalVisibilityDemoNamePrompt.value=undefined}
export function showTerminalVisibilityDemoContextMenu(id:string,x:number,y:number){if(id!=='default')terminalVisibilityDemoContextMenu.value={id,x,y}}
export function promptRenameTerminalVisibilityDemoGroup(){const id=terminalVisibilityDemoContextMenu.value?.id,group=terminalVisibilityDemoState.value.groups.find(item=>item.id===id);terminalVisibilityDemoContextMenu.value=undefined;if(group)terminalVisibilityDemoNamePrompt.value={mode:'rename',groupId:group.id,value:group.name}}
export function cancelTerminalVisibilityDemoName(){terminalVisibilityDemoNamePrompt.value=undefined}
export function renameTerminalVisibilityDemoGroup(name:string){terminalVisibilityDemoState.value=renameTerminalVisibilityGroup(terminalVisibilityDemoState.value,active().id,name)}
export function removeTerminalVisibilityDemoGroup(){const id=terminalVisibilityDemoContextMenu.value?.id??active().id;terminalVisibilityDemoContextMenu.value=undefined;terminalVisibilityDemoState.value=removeTerminalVisibilityGroup(terminalVisibilityDemoState.value,id)}
export function toggleTerminalVisibilityDemo(key:string){terminalVisibilityDemoState.value=setTerminalVisibleInGroup(terminalVisibilityDemoState.value,active().id,key,active().hiddenKeys.includes(key))}
export function setAllTerminalVisibilityDemo(visible:boolean){terminalVisibilityDemoState.value=setAllTerminalsVisibleInGroup(terminalVisibilityDemoState.value,active().id,terminalVisibilityDemoGroups.flatMap(group=>group.sessions.map(session=>`${session.projectId}:${session.id}`)),visible)}
