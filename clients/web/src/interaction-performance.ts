export type InteractionName='ticket-view-change'|'workspace-mode-change'|'project-change'|'ticket-selection'|'ticket-up-next-change'|'ticket-status-change'|'ticket-change'|'bulk-ticket-change'|'permission-decision';

export interface InteractionTiming {
  name:InteractionName;
  update_ms:number;
  paint_ms:number;
  budget_ms:number;
  over_budget:boolean;
  detail?:Record<string,string|number|boolean>;
}

const INTERACTION_BUDGET_MS=100;

/** Measure from an intent handler into the task after its next frame, when the update has painted. */
export function beginInteractionTiming(name:InteractionName,detail?:InteractionTiming['detail']){
  const started=performance.now();let finished=false;
  return()=>{if(finished)return;finished=true;const updated=performance.now();requestAnimationFrame(()=>setTimeout(()=>{const painted=performance.now(),timing:InteractionTiming={name,update_ms:updated-started,paint_ms:painted-started,budget_ms:INTERACTION_BUDGET_MS,over_budget:painted-started>INTERACTION_BUDGET_MS,...detail?{detail}:{}};document.dispatchEvent(new CustomEvent<InteractionTiming>('hotsheet:interaction-timing',{detail:timing}))},0))};
}
