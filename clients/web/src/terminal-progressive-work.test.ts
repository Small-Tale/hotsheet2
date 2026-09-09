import { describe, expect, it } from 'vitest';

import { ProgressiveTerminalWorkQueue } from './terminal-progressive-work';

describe('ProgressiveTerminalWorkQueue', () => {
  it('bounds mounting and disposal across scheduled turns', () => {
    const scheduled:Array<() => void>=[],mounted:number[]=[],disposed:number[]=[];
    const queue=new ProgressiveTerminalWorkQueue<number>({mount:item=>{mounted.push(item)},dispose:work=>{work()},schedule:work=>{scheduled.push(work)},mountsPerTurn:2,disposalsPerTurn:1});
    for(let item=1;item<=5;item+=1)queue.enqueueMount(item);
    queue.enqueueDisposal(()=>disposed.push(1));queue.enqueueDisposal(()=>disposed.push(2));
    expect({mounted,disposed,scheduled:scheduled.length}).toEqual({mounted:[],disposed:[],scheduled:1});
    scheduled.shift()!();
    expect({mounted,disposed,scheduled:scheduled.length}).toEqual({mounted:[1,2],disposed:[1],scheduled:1});
    scheduled.shift()!();scheduled.shift()!();
    expect({mounted,disposed,scheduled:scheduled.length}).toEqual({mounted:[1,2,3,4,5],disposed:[1,2],scheduled:0});
  });

  it('cancels a deferred mount without disturbing later work', () => {
    const scheduled:Array<() => void>=[],mounted:string[]=[];
    const queue=new ProgressiveTerminalWorkQueue<string>({mount:item=>{mounted.push(item)},dispose:work=>{work()},schedule:work=>{scheduled.push(work)},mountsPerTurn:1});
    queue.enqueueMount('offscreen');queue.enqueueMount('visible');queue.cancelMount('offscreen');
    scheduled.shift()!();
    expect(mounted).toEqual(['visible']);
    expect(queue.pendingMountCount).toBe(0);
  });
});
