import {describe,expect,it,vi} from 'vitest';

import {type GalleryVideoFrameMetadata,GalleryVideoPlaybackController,type GalleryVideoPlaybackResource} from './gallery-video-playback';

class FakeVideo implements GalleryVideoPlaybackResource {
  #currentTime=0;
  duration=10;
  muted=false;
  paused=true;
  readyState=1;
  playCalls=0;
  pauseCalls=0;
  playResult:Promise<void>=Promise.resolve();
  assignments:number[]=[];
  listeners=new Set<()=>void>();
  frames=new Map<number,(now:number,metadata:GalleryVideoFrameMetadata)=>void>();
  nextFrame=1;
  get currentTime(){return this.#currentTime}
  set currentTime(seconds:number){this.#currentTime=seconds;this.assignments.push(seconds)}
  addEventListener(_type:'seeked',listener:()=>void){this.listeners.add(listener)}
  removeEventListener(_type:'seeked',listener:()=>void){this.listeners.delete(listener)}
  requestVideoFrameCallback(callback:(now:number,metadata:GalleryVideoFrameMetadata)=>void){const id=this.nextFrame++;this.frames.set(id,callback);return id}
  cancelVideoFrameCallback(id:number){this.frames.delete(id)}
  play(){this.playCalls+=1;this.paused=false;return this.playResult}
  pause(){this.pauseCalls+=1;this.paused=true}
  seeked(){for(const listener of this.listeners)listener()}
  frame(){const entry=this.frames.entries().next().value;if(!entry)return;this.frames.delete(entry[0]);entry[1](0,{mediaTime:this.currentTime})}
}

function controlledVideo(){
  return new FakeVideo();
}

describe('GalleryVideoPlaybackController',()=>{
  it('coalesces rapid seeks and presents only the newest decoded target',()=>{
    const video=controlledVideo(),present=vi.fn(),controller=new GalleryVideoPlaybackController(video,present);
    controller.seek(1000);controller.seek(4000);controller.seek(8000);
    expect(video.assignments).toEqual([1]);expect(controller.busy).toBe(true);
    video.seeked();expect(video.assignments).toEqual([1,8]);expect(present).not.toHaveBeenCalled();
    video.seeked();video.frame();expect(present).toHaveBeenCalledOnce();expect(present).toHaveBeenCalledWith(8000);expect(controller.busy).toBe(false);
  });

  it('observes the decoded frame before seeked so paused scrubs do not stall',()=>{
    const video=controlledVideo(),present=vi.fn(),controller=new GalleryVideoPlaybackController(video,present);
    controller.seek(2500);
    expect(video.frames.size).toBe(1);
    video.frame();
    expect(controller.busy).toBe(true);
    video.seeked();
    expect(present).toHaveBeenCalledWith(2500);
    expect(controller.busy).toBe(false);
    controller.seek(7000);
    expect(video.assignments).toEqual([2.5,7]);
  });

  it('replaces a target queued while the prior decoded frame is pending',()=>{
    const video=controlledVideo(),present=vi.fn(),controller=new GalleryVideoPlaybackController(video,present);
    controller.seek(2000);video.seeked();controller.seek(7000);video.frame();
    expect(video.assignments).toEqual([2,7]);expect(present).not.toHaveBeenCalled();
    video.seeked();video.frame();expect(present).toHaveBeenCalledWith(7000);
  });

  it('briefly plays while decoding the first frame, then pauses and restores mute',()=>{
    const video=controlledVideo(),present=vi.fn(),controller=new GalleryVideoPlaybackController(video,present);
    video.readyState=2;controller.primeFirstFrame();expect(video.playCalls).toBe(1);expect(video.muted).toBe(true);expect(video.assignments).toEqual([.001]);expect(controller.busy).toBe(true);
    video.frame();video.seeked();expect(video.pauseCalls).toBe(1);expect(video.muted).toBe(false);expect(present).toHaveBeenCalledWith(0);expect(controller.busy).toBe(false);
  });

  it('issues the newest scrub target after decoder priming and cancels priming on disposal',()=>{
    const video=controlledVideo(),present=vi.fn(),controller=new GalleryVideoPlaybackController(video,present);
    video.muted=true;controller.primeFirstFrame();controller.seek(3000);controller.seek(7000);expect(video.assignments).toEqual([.001]);video.frame();expect(video.pauseCalls).toBe(0);expect(video.muted).toBe(true);expect(video.assignments).toEqual([.001,7]);
    video.seeked();video.frame();expect(present).toHaveBeenCalledWith(7000);expect(video.pauseCalls).toBe(1);
    controller.primeFirstFrame();expect(video.playCalls).toBe(2);controller.dispose();expect(video.frames.size).toBe(0);expect(video.listeners.size).toBe(0);video.frame();expect(present).toHaveBeenCalledOnce();controller.seek(3000);expect(video.assignments).toEqual([.001,7,.001]);
  });

  it('retains paused seeking when internal playback is blocked',async()=>{
    const video=controlledVideo(),present=vi.fn(),controller=new GalleryVideoPlaybackController(video,present);video.playResult=Promise.reject(new Error('blocked'));
    controller.primeFirstFrame();await video.playResult.catch(()=>undefined);await Promise.resolve();expect(video.assignments).toEqual([.001]);expect(video.frames.size).toBe(1);
    controller.dispose();expect(video.frames.size).toBe(0);expect(video.listeners.size).toBe(0);video.frame();expect(present).not.toHaveBeenCalled();controller.seek(3000);expect(video.assignments).toEqual([.001]);
  });

  it('hands internal decoder playback to an immediate user play action',()=>{
    const video=controlledVideo(),controller=new GalleryVideoPlaybackController(video,vi.fn());controller.seek(4000);expect(controller.internalPlayback).toBe(true);expect(video.paused).toBe(false);expect(video.muted).toBe(true);
    controller.prepareUserPlayback();expect(controller.internalPlayback).toBe(false);expect(video.paused).toBe(true);expect(video.muted).toBe(false);
  });
});
