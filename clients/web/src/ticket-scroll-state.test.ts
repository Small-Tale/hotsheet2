import {describe,expect,it} from 'vitest';

import {captureTicketScrollState,restoreTicketScrollState} from './ticket-scroll-state';

describe('ticket scroll state',()=>{
  it('restores every named ticket scroll owner after its contents move',()=>{
    const owners=[{dataset:{ticketScrollOwner:'list'},scrollTop:420,scrollLeft:0},{dataset:{ticketScrollOwner:'column:started'},scrollTop:180,scrollLeft:12}].map(owner=>Object.assign(owner,{scrollTo(left:number,top:number){owner.scrollLeft=left;owner.scrollTop=top}})) as unknown as HTMLElement[];
    const root={querySelectorAll:()=>owners} as unknown as ParentNode;
    const state=captureTicketScrollState(root);
    for(const owner of owners){owner.scrollTop=0;owner.scrollLeft=0}
    restoreTicketScrollState(state,root);
    expect(owners.map(owner=>[owner.scrollLeft,owner.scrollTop])).toEqual([[0,420],[12,180]]);
  });
});
