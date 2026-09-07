import {describe,expect,it,vi} from 'vitest';

import {animateTicketMotion,type TicketMotionSnapshot} from './ticket-motion';

describe('ticket motion',()=>{
  it('uses FLIP for retained rows and fades incoming rows after space opens',()=>{const animate=vi.fn(),old={getBoundingClientRect:()=>({left:10,top:20} as DOMRect),closest:()=>({dataset:{columnId:'started'}})} as unknown as HTMLElement,current={dataset:{ticketSlug:'HS2-A'},getBoundingClientRect:()=>({left:110,top:80} as DOMRect),closest:()=>({dataset:{columnId:'completed'}}),animate} as unknown as HTMLElement,incoming={dataset:{ticketSlug:'HS2-B'},getBoundingClientRect:()=>({left:110,top:140} as DOMRect),closest:()=>({dataset:{columnId:'completed'}}),animate} as unknown as HTMLElement,root={querySelector:()=>({dataset:{presentation:'edge-to-edge'},querySelector:()=>({dataset:{component:'ticket-board'}})}),querySelectorAll:()=>[current,incoming]} as unknown as ParentNode,snapshot:TicketMotionSnapshot={scope:'edge-to-edge:ticket-board',rows:new Map([['HS2-A',{rect:old.getBoundingClientRect(),parent:'started',element:old}]])};animateTicketMotion(snapshot,root,false);expect(animate).toHaveBeenCalledTimes(2);expect(animate.mock.calls[0][0][0]).toMatchObject({transform:'translate(-100px, -60px)',zIndex:'30'});expect(animate.mock.calls[1][0]).toMatchObject([{opacity:0},{opacity:0,offset:.45},{opacity:1}])});
});
