export interface GalleryVideoFrameMetadata {mediaTime:number}

export interface GalleryVideoPlaybackResource {
  currentTime:number;
  duration:number;
  readyState:number;
  addEventListener(type:'seeked',listener:()=>void):void;
  removeEventListener(type:'seeked',listener:()=>void):void;
  requestVideoFrameCallback?(callback:(now:number,metadata:GalleryVideoFrameMetadata)=>void):number;
  cancelVideoFrameCallback?(handle:number):void;
}

/** Serializes expensive decoder seeks while retaining the newest pointer target. */
export class GalleryVideoPlaybackController {
  readonly #video:GalleryVideoPlaybackResource;
  readonly #present:(milliseconds:number)=>void;
  #pending:number|undefined;
  #target:number|undefined;
  #frame:number|undefined;
  #disposed=false;

  constructor(video:GalleryVideoPlaybackResource,present:(milliseconds:number)=>void){
    this.#video=video;
    this.#present=present;
    video.addEventListener('seeked',this.#seeked);
  }

  get busy(){return this.#pending!==undefined||this.#target!==undefined||this.#frame!==undefined}

  seek(milliseconds:number){
    if(this.#disposed)return;
    this.#pending=Math.max(0,Math.round(milliseconds));
    if(this.#target===undefined&&this.#frame===undefined)this.#issue();
  }

  /** Metadata alone does not require a browser to decode a frame. A tiny initial seek does. */
  primeFirstFrame(){
    if(this.#disposed||this.#video.readyState>=2)return;
    this.#pending=0;
    this.#issue(true);
  }

  dispose(){
    this.#disposed=true;
    this.#pending=undefined;
    this.#target=undefined;
    if(this.#frame!==undefined)this.#video.cancelVideoFrameCallback?.(this.#frame);
    this.#frame=undefined;
    this.#video.removeEventListener('seeked',this.#seeked);
  }

  #issue(prime=false){
    const target=this.#pending;
    if(target===undefined||this.#disposed)return;
    this.#pending=undefined;
    this.#target=target;
    const seconds=prime&&target===0?Math.min(.001,Math.max(0,this.#video.duration)):target/1000;
    this.#video.currentTime=seconds;
  }

  #seeked=()=>{
    if(this.#disposed||this.#target===undefined)return;
    if(this.#pending!==undefined){this.#target=undefined;this.#issue();return}
    const request=this.#video.requestVideoFrameCallback?.bind(this.#video);
    if(!request){this.#finish();return}
    this.#frame=request(()=>{
      this.#frame=undefined;
      if(this.#disposed)return;
      if(this.#pending!==undefined){this.#target=undefined;this.#issue();return}
      this.#finish();
    });
  };

  #finish(){
    const target=this.#target;
    this.#target=undefined;
    if(target!==undefined)this.#present(target);
    if(this.#pending!==undefined)this.#issue();
  }
}
