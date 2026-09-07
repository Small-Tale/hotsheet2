export const DEFAULT_TERMINAL_VISIBILITY_GROUP_ID='default';
export const DEFAULT_TERMINAL_VISIBILITY_GROUP_NAME='Default';
export const TERMINAL_VISIBILITY_STORAGE_KEY='hotsheet.terminals.visibility-groups';
export const TERMINAL_DASHBOARD_VISIBILITY_SCOPE='dashboard';

export interface TerminalVisibilityGroup {id:string;name:string;hiddenKeys:string[]}
export interface TerminalVisibilityState {groups:TerminalVisibilityGroup[];activeByScope:Record<string,string>}

export const terminalProjectVisibilityScope=(projectId:string)=>`project:${projectId}`;
export function initialTerminalVisibilityState():TerminalVisibilityState{return{groups:[{id:DEFAULT_TERMINAL_VISIBILITY_GROUP_ID,name:DEFAULT_TERMINAL_VISIBILITY_GROUP_NAME,hiddenKeys:[]}],activeByScope:{}}}

export function parseTerminalVisibilityState(raw:string|null):TerminalVisibilityState{
  if(!raw)return initialTerminalVisibilityState();
  try{
    const value=JSON.parse(raw) as {groups?:unknown;activeByScope?:unknown};
    if(!Array.isArray(value.groups))return initialTerminalVisibilityState();
    const seen=new Set<string>(),groups:TerminalVisibilityGroup[]=[];
    for(const item of value.groups){
      if(!item||typeof item!=='object')continue;
      const candidate=item as {id?:unknown;name?:unknown;hiddenKeys?:unknown};
      if(typeof candidate.id!=='string'||!candidate.id||seen.has(candidate.id)||typeof candidate.name!=='string'||!candidate.name.trim())continue;
      const hiddenKeys=Array.isArray(candidate.hiddenKeys)?[...new Set(candidate.hiddenKeys.filter((key):key is string=>typeof key==='string'&&Boolean(key)))]:[];
      seen.add(candidate.id);groups.push({id:candidate.id,name:candidate.name.trim(),hiddenKeys});
    }
    if(!groups.some(group=>group.id===DEFAULT_TERMINAL_VISIBILITY_GROUP_ID))groups.unshift({id:DEFAULT_TERMINAL_VISIBILITY_GROUP_ID,name:DEFAULT_TERMINAL_VISIBILITY_GROUP_NAME,hiddenKeys:[]});
    const activeByScope:Record<string,string>={},valid=new Set(groups.map(group=>group.id));
    if(value.activeByScope&&typeof value.activeByScope==='object'&&!Array.isArray(value.activeByScope))for(const[scope,id]of Object.entries(value.activeByScope))if(scope&&typeof id==='string'&&id!==DEFAULT_TERMINAL_VISIBILITY_GROUP_ID&&valid.has(id))activeByScope[scope]=id;
    return{groups,activeByScope};
  }catch{return initialTerminalVisibilityState()}
}

export function activeTerminalVisibilityGroup(state:TerminalVisibilityState,scope:string):TerminalVisibilityGroup{
  const id=state.activeByScope[scope]??DEFAULT_TERMINAL_VISIBILITY_GROUP_ID;
  return state.groups.find(group=>group.id===id)??state.groups.find(group=>group.id===DEFAULT_TERMINAL_VISIBILITY_GROUP_ID)??initialTerminalVisibilityState().groups[0];
}

export function selectTerminalVisibilityGroup(state:TerminalVisibilityState,scope:string,id:string):TerminalVisibilityState{
  if(!state.groups.some(group=>group.id===id))return state;
  const activeByScope=id===DEFAULT_TERMINAL_VISIBILITY_GROUP_ID
    ?Object.fromEntries(Object.entries(state.activeByScope).filter(([candidateScope])=>candidateScope!==scope))
    :{...state.activeByScope,[scope]:id};
  return{...state,activeByScope};
}

export function addTerminalVisibilityGroup(state:TerminalVisibilityState,id:string,name='New group'):{state:TerminalVisibilityState;group:TerminalVisibilityGroup}{
  const unique=id&&!state.groups.some(group=>group.id===id)?id:`group-${state.groups.length+1}`;
  const group={id:unique,name:name.trim()||'New group',hiddenKeys:[]};
  return{state:{...state,groups:[...state.groups,group]},group};
}

export function renameTerminalVisibilityGroup(state:TerminalVisibilityState,id:string,name:string):TerminalVisibilityState{
  const trimmed=name.trim();if(!trimmed||id===DEFAULT_TERMINAL_VISIBILITY_GROUP_ID)return state;
  return{...state,groups:state.groups.map(group=>group.id===id?{...group,name:trimmed}:group)};
}

export function removeTerminalVisibilityGroup(state:TerminalVisibilityState,id:string):TerminalVisibilityState{
  if(id===DEFAULT_TERMINAL_VISIBILITY_GROUP_ID||!state.groups.some(group=>group.id===id))return state;
  return{groups:state.groups.filter(group=>group.id!==id),activeByScope:Object.fromEntries(Object.entries(state.activeByScope).filter(([,active])=>active!==id))};
}

export function setTerminalVisibleInGroup(state:TerminalVisibilityState,groupId:string,key:string,visible:boolean):TerminalVisibilityState{
  return{...state,groups:state.groups.map(group=>{
    if(group.id!==groupId)return group;
    const hidden=group.hiddenKeys.includes(key);
    if(hidden===!visible)return group;
    return{...group,hiddenKeys:visible?group.hiddenKeys.filter(item=>item!==key):[...group.hiddenKeys,key]};
  })};
}

export function setAllTerminalsVisibleInGroup(state:TerminalVisibilityState,groupId:string,keys:readonly string[],visible:boolean):TerminalVisibilityState{
  return keys.reduce((next,key)=>setTerminalVisibleInGroup(next,groupId,key,visible),state);
}

export function hideNewTerminalInNamedGroups(state:TerminalVisibilityState,key:string):TerminalVisibilityState{
  return{...state,groups:state.groups.map(group=>group.id===DEFAULT_TERMINAL_VISIBILITY_GROUP_ID||group.hiddenKeys.includes(key)?group:{...group,hiddenKeys:[...group.hiddenKeys,key]})};
}
