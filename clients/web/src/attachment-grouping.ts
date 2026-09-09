import type {Attachment,Note} from './api';

const statusChange=(note:Note)=>note.kind==='activity'&&/^Status changed from\b/.test(note.summary??note.text);
const time=(value:string)=>Number.isFinite(Date.parse(value))?Date.parse(value):0;

/** Reuse the current human round until a durable ticket status transition occurs. */
export function attachmentUploadBatchId(attachments:readonly Attachment[],notes:readonly Note[],createId:()=>string):string {
  const latest=[...attachments].sort((left,right)=>time(right.created_at)-time(left.created_at)).at(0);
  if(!latest?.batch_id||latest.actor?.role!=='human')return createId();
  const changedAfterUpload=notes.some(note=>statusChange(note)&&time(note.created_at)>time(latest.created_at));
  return changedAfterUpload?createId():latest.batch_id;
}

/** Number only attachment-bearing workflow phases; state changes without evidence do not create empty rounds. */
export function attachmentRoundNumbers(attachments:readonly Attachment[],notes:readonly Note[]):Map<string,number> {
  const ordered=[...attachments].sort((left,right)=>time(left.created_at)-time(right.created_at));
  const changes=notes.filter(statusChange).map(note=>time(note.created_at)).sort((left,right)=>left-right);
  const rounds=new Map<string,number>();let round=1,previous=ordered[0]?time(ordered[0].created_at):0;
  for(const attachment of ordered){const current=time(attachment.created_at);if(rounds.size>0&&changes.some(change=>change>previous&&change<=current))round+=1;rounds.set(attachment.id,round);previous=current}
  return rounds;
}
