export type PermissionDecision = 'allow'|'deny';
export type PermissionScope = 'once'|'always';
export type PermissionHistoryDecision = PermissionDecision|'external';
export type PermissionAutomationAction = 'off'|'allow'|'deny';

export interface WirePermissionRequest { id:number;connection:string;tool:string;action:string;always_allow_supported?:boolean }
export interface ToolConnection { id:string;tool:string;project:string;role:'main'|'worker'|'drivespawned';busy:boolean }
export interface PermissionProject { id:string;name:string;root:string;apiPath:string }
export interface PermissionItem extends WirePermissionRequest { key:string;projectId:string;projectName:string;agent:string;role:string;receivedAt:number;ignored:boolean }
export interface PermissionHistoryItem extends PermissionItem { decision:PermissionHistoryDecision;scope?:PermissionScope;resolvedAt:number;automatic?:boolean }
export interface PermissionAutomation { action:PermissionAutomationAction;delayMs:number }

export const PERMISSION_DELAYS = [15_000,60_000,120_000,300_000,900_000,3_600_000] as const;
export const DEFAULT_PERMISSION_AUTOMATION:PermissionAutomation={action:'off',delayMs:60_000};

export function parsePermissionAutomation(raw:unknown):PermissionAutomation {
  if(!raw||typeof raw!=='object')return DEFAULT_PERMISSION_AUTOMATION;
  const value=raw as Partial<PermissionAutomation>;
  return {action:value.action==='allow'||value.action==='deny'?value.action:'off',delayMs:PERMISSION_DELAYS.includes(value.delayMs as typeof PERMISSION_DELAYS[number])?value.delayMs!:60_000};
}

export function parsePermissionHistory(raw:unknown):PermissionHistoryItem[] {
  if(!Array.isArray(raw))return [];
  return raw.filter((item):item is PermissionHistoryItem=>Boolean(item)&&typeof item==='object'&&typeof (item as PermissionHistoryItem).key==='string'&&typeof (item as PermissionHistoryItem).resolvedAt==='number').slice(-200);
}

export function formatPermissionCountdown(milliseconds:number):string {const total=Math.ceil(Math.max(0,milliseconds)/1000);return `${Math.floor(total/60)}:${String(total%60).padStart(2,'0')}`}

export function parsePermissionResolution(message?:string):{decision:PermissionDecision;scope:PermissionScope}|undefined {
  const [decision,scope]=message?.split(':')??[];
  if(decision!=='allow'&&decision!=='deny')return undefined;
  return {decision,scope:scope==='always'?'always':'once'};
}

export class PermissionInbox {
  private pendingItems=new Map<string,PermissionItem>();
  private historyItems:PermissionHistoryItem[];
  constructor(history:unknown=[]){this.historyItems=parsePermissionHistory(history)}
  pending(){return [...this.pendingItems.values()].sort((a,b)=>a.receivedAt-b.receivedAt)}
  history(){return [...this.historyItems].sort((a,b)=>b.resolvedAt-a.resolvedAt)}
  visible(){return this.pending().find(item=>!item.ignored)}
  reconcile(project:PermissionProject,requests:WirePermissionRequest[],connections:ToolConnection[],now=Date.now()){
    let changed=false;
    const currentKeys=new Set(requests.map(request=>`${project.id}:${request.id}`));
    for(const item of this.pending())if(item.projectId===project.id&&!currentKeys.has(item.key)){this.pendingItems.delete(item.key);this.record({...item,decision:'external',resolvedAt:now});changed=true}
    for(const request of requests){const key=`${project.id}:${request.id}`,existing=this.pendingItems.get(key),connection=connections.find(item=>item.id===request.connection);if(!existing){this.pendingItems.set(key,{...request,key,projectId:project.id,projectName:project.name,agent:connection?.tool||friendlyAgent(request.tool),role:connection?.role==='worker'?'worker':'main worker',receivedAt:now,ignored:false});changed=true;continue}const next={...existing,...request,projectName:project.name,agent:connection?.tool||existing.agent,role:connection?.role==='worker'?'worker':existing.role};if(existing.connection!==next.connection||existing.tool!==next.tool||existing.action!==next.action||existing.always_allow_supported!==next.always_allow_supported||existing.projectName!==next.projectName||existing.agent!==next.agent||existing.role!==next.role){this.pendingItems.set(key,next);changed=true}}
    return changed;
  }
  ignore(key:string){const item=this.pendingItems.get(key);if(item)this.pendingItems.set(key,{...item,ignored:true})}
  present(key:string){const item=this.pendingItems.get(key);if(item)this.pendingItems.set(key,{...item,ignored:false})}
  resolve(key:string,decision:PermissionDecision,scope:PermissionScope,automatic=false,now=Date.now()){const item=this.pendingItems.get(key);if(!item)return;this.pendingItems.delete(key);this.record({...item,decision,scope,resolvedAt:now,automatic})}
  private record(item:PermissionHistoryItem){this.historyItems=[...this.historyItems.filter(value=>value.key!==item.key),item].slice(-200)}
}

export class VisiblePermissionTimer {
  private remaining=new Map<string,number>();private active?:{key:string;at:number};private cancelled=new Set<string>();
  show(key:string,delayMs:number,now=Date.now()){if(!this.remaining.has(key))this.remaining.set(key,delayMs);this.active={key,at:now}}
  hide(now=Date.now()){this.consume(now);this.active=undefined}
  cancel(key:string,now=Date.now()){this.consume(now);this.cancelled.add(key);if(this.active?.key===key)this.active=undefined}
  remove(key:string){this.remaining.delete(key);this.cancelled.delete(key);if(this.active?.key===key)this.active=undefined}
  tick(key:string,delayMs:number,now=Date.now()){if(this.cancelled.has(key))return undefined;if(this.active?.key!==key){this.hide(now);this.show(key,delayMs,now)}this.consume(now);if(this.active)this.active.at=now;return this.remaining.get(key)??delayMs}
  private consume(now:number){if(!this.active)return;const value=this.remaining.get(this.active.key)??0;this.remaining.set(this.active.key,Math.max(0,value-(now-this.active.at)));this.active.at=now}
}

function friendlyAgent(tool:string){const value=tool.toLowerCase();if(value.includes('codex'))return 'Codex';if(value.includes('claude')||['bash','edit','write','read'].includes(value))return 'Claude';return tool||'AI tool'}
