import { signal } from 'kerfjs';

import { TerminalVisibilityDialog } from '../components/terminal-visibility-dialog';
import { activeTerminalVisibilityGroup,addTerminalVisibilityGroup,initialTerminalVisibilityState,removeTerminalVisibilityGroup,renameTerminalVisibilityGroup,selectTerminalVisibilityGroup,setAllTerminalsVisibleInGroup,setTerminalVisibleInGroup } from '../terminal-visibility';

export const terminalVisibilityDemoGroups=[{projectId:'demo',projectName:'Demo project',sessions:[{id:'ai',projectId:'demo',projectName:'Demo project',title:'AI',alive:true,busy:true,scrollback:''},{id:'server',projectId:'demo',projectName:'Demo project',title:'Development server',alive:true,busy:false,scrollback:''}]}];
const seeded=addTerminalVisibilityGroup(initialTerminalVisibilityState(),'focus','Focus');
export const terminalVisibilityDemoState=signal(setTerminalVisibleInGroup(selectTerminalVisibilityGroup(seeded.state,'dashboard','focus'),'focus','demo:server',false));
export function TerminalVisibilityDialogDemo(){return <TerminalVisibilityDialog open state={terminalVisibilityDemoState.value} scope="dashboard" groups={terminalVisibilityDemoGroups}/>}
const active=()=>activeTerminalVisibilityGroup(terminalVisibilityDemoState.value,'dashboard');
export function selectTerminalVisibilityDemoGroup(id:string){terminalVisibilityDemoState.value=selectTerminalVisibilityGroup(terminalVisibilityDemoState.value,'dashboard',id)}
export function addTerminalVisibilityDemoGroup(){const added=addTerminalVisibilityGroup(terminalVisibilityDemoState.value,`demo-${terminalVisibilityDemoState.value.groups.length}`);terminalVisibilityDemoState.value=selectTerminalVisibilityGroup(added.state,'dashboard',added.group.id)}
export function renameTerminalVisibilityDemoGroup(name:string){terminalVisibilityDemoState.value=renameTerminalVisibilityGroup(terminalVisibilityDemoState.value,active().id,name)}
export function removeTerminalVisibilityDemoGroup(){terminalVisibilityDemoState.value=removeTerminalVisibilityGroup(terminalVisibilityDemoState.value,active().id)}
export function toggleTerminalVisibilityDemo(key:string){terminalVisibilityDemoState.value=setTerminalVisibleInGroup(terminalVisibilityDemoState.value,active().id,key,active().hiddenKeys.includes(key))}
export function setAllTerminalVisibilityDemo(visible:boolean){terminalVisibilityDemoState.value=setAllTerminalsVisibleInGroup(terminalVisibilityDemoState.value,active().id,terminalVisibilityDemoGroups.flatMap(group=>group.sessions.map(session=>`${session.projectId}:${session.id}`)),visible)}
