import {describe,expect,it,vi} from 'vitest';

import {createRefreshBarrier} from './refresh-barrier';

describe('refresh barrier',()=>{
  it('holds refreshes until an active local mutation finishes',async()=>{
    const barrier=createRefreshBarrier(),release=barrier.begin(),settled=vi.fn();
    const waiting=barrier.wait().then(settled);
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    release();
    await waiting;
    expect(settled).toHaveBeenCalledOnce();
  });

  it('waits for every overlapping mutation and tolerates repeated release',async()=>{
    const barrier=createRefreshBarrier(),releaseFirst=barrier.begin(),releaseSecond=barrier.begin(),settled=vi.fn();
    const waiting=barrier.wait().then(settled);
    releaseFirst();
    releaseFirst();
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    releaseSecond();
    await waiting;
    expect(settled).toHaveBeenCalledOnce();
  });
});
