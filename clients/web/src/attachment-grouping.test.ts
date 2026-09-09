import {describe,expect,it} from 'vitest';

import type {Attachment,Note} from './api';
import {attachmentRoundNumbers,attachmentUploadBatchId} from './attachment-grouping';

const attachment=(created_at:string,overrides:Partial<Attachment>={}):Attachment=>({id:'a',filename:'proof.png',created_at,batch_id:'human-round',actor:{role:'human'},annotations:[],...overrides});
const activity=(created_at:string,text:string):Note=>({id:created_at,kind:'activity',created_at,edited_at:created_at,text});

describe('attachment grouping rounds',()=>{
  it('keeps separate uploads in one human round while ticket state is unchanged',()=>{
    expect(attachmentUploadBatchId([attachment('2026-09-09T01:00:00Z')],[activity('2026-09-09T01:05:00Z','Claim renewed')],()=> 'new-round')).toBe('human-round');
  });
  it('starts a new round only after a status transition',()=>{
    expect(attachmentUploadBatchId([attachment('2026-09-09T01:00:00Z')],[activity('2026-09-09T01:05:00Z','Status changed from Started to Completed')],()=> 'new-round')).toBe('new-round');
  });
  it('starts a human round when the latest attachment lacks compatible provenance',()=>{
    expect(attachmentUploadBatchId([attachment('2026-09-09T01:00:00Z',{actor:{role:'ai'}})],[],()=> 'new-round')).toBe('new-round');
    expect(attachmentUploadBatchId([],[],()=> 'first-round')).toBe('first-round');
  });
  it('numbers a new visible round only when a state change falls between attachments',()=>{
    const attachments=[attachment('2026-09-09T01:00:00Z',{id:'one'}),attachment('2026-09-09T01:10:00Z',{id:'two',batch_id:'another-upload'}),attachment('2026-09-09T02:00:00Z',{id:'three',batch_id:'after-review'})];
    const notes=[activity('2026-09-09T01:05:00Z','Claim renewed'),activity('2026-09-09T01:30:00Z','Status changed from Started to Completed')];
    expect([...attachmentRoundNumbers(attachments,notes)]).toEqual([['one',1],['two',1],['three',2]]);
  });
});
