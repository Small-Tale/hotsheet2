import {describe,expect,it,vi} from 'vitest';

import {type GalleryVideoFrameMetadata,GalleryVideoPlaybackController,type GalleryVideoPlaybackResource} from './gallery-video-playback';

class FakeVideo implements GalleryVideoPlaybackResource {
  #currentTime=0;
  duration=10;
  readyState=1;
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

  it('replaces a target queued while the prior decoded frame is pending',()=>{
    const video=controlledVideo(),present=vi.fn(),controller=new GalleryVideoPlaybackController(video,present);
    controller.seek(2000);video.seeked();controller.seek(7000);video.frame();
    expect(video.assignments).toEqual([2,7]);expect(present).not.toHaveBeenCalled();
    video.seeked();video.frame();expect(present).toHaveBeenCalledWith(7000);
  });

  it('primes a paused first frame and cancels stale callbacks on disposal',()=>{
    const video=controlledVideo(),present=vi.fn(),controller=new GalleryVideoPlaybackController(video,present);
    controller.primeFirstFrame();expect(video.assignments).toEqual([.001]);video.seeked();expect(video.frames.size).toBe(1);
    controller.dispose();expect(video.frames.size).toBe(0);expect(video.listeners.size).toBe(0);video.frame();expect(present).not.toHaveBeenCalled();controller.seek(3000);expect(video.assignments).toEqual([.001]);
  });

  it('does not prime when the browser already has a decoded frame',()=>{
    const video=controlledVideo();video.readyState=2;const controller=new GalleryVideoPlaybackController(video,vi.fn());controller.primeFirstFrame();expect(video.assignments).toEqual([]);expect(controller.busy).toBe(false);
  });
});
