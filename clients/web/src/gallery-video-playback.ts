export interface GalleryVideoFrameMetadata {mediaTime:number}

export interface GalleryVideoPlaybackResource {
  currentTime:number;
  duration:number;
  muted:boolean;
  paused:boolean;
  play():Promise<void>;
  pause():void;
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
  #pauseAfterDecode=false;
  #restoreMuted=false;
  #framePresented=false;
  #seekCompleted=false;
  #disposed=false;

  constructor(video:GalleryVideoPlaybackResource,present:(milliseconds:number)=>void){
    this.#video=video;
    this.#present=present;
    video.addEventListener('seeked',this.#seeked);
  }

  get busy(){return this.#pending!==undefined||this.#target!==undefined||this.#frame!==undefined}
  get internalPlayback(){return this.#pauseAfterDecode}

  prepareUserPlayback(){this.#stopInternalPlayback()}

  seek(milliseconds:number){
    if(this.#disposed)return;
    this.#pending=Math.max(0,Math.round(milliseconds));
    if(this.#target===undefined&&this.#frame===undefined)this.#issue();
  }

  /** Prime the same active-decoding path used by paused scrubs at the first frame. */
  primeFirstFrame(){
    if(this.#disposed||this.#target!==undefined||this.#frame!==undefined)return;
    if(this.#pending===undefined)this.#pending=0;
    this.#issue(this.#pending===0);
  }

  dispose(){
    this.#disposed=true;
    this.#pending=undefined;
    this.#target=undefined;
    if(this.#frame!==undefined)this.#video.cancelVideoFrameCallback?.(this.#frame);
    this.#frame=undefined;
    this.#stopInternalPlayback();
    this.#video.removeEventListener('seeked',this.#seeked);
  }

  #startInternalPlayback(){
    if(!this.#video.paused||this.#pauseAfterDecode)return;
    this.#pauseAfterDecode=true;
    this.#restoreMuted=this.#video.muted;
    this.#video.muted=true;
    try{void this.#video.play().catch(()=>{this.#stopInternalPlayback()})}catch{this.#stopInternalPlayback()}
  }

  #stopInternalPlayback(){
    if(!this.#pauseAfterDecode)return;
    this.#pauseAfterDecode=false;
    if(!this.#video.paused)this.#video.pause();
    this.#video.muted=this.#restoreMuted;
  }

  #issue(prime=false){
    const target=this.#pending;
    if(target===undefined||this.#disposed)return;
    this.#pending=undefined;
    this.#target=target;
    const seconds=prime&&target===0?Math.min(.001,Math.max(0,this.#video.duration)):target/1000;
    this.#framePresented=false;
    this.#seekCompleted=false;
    this.#startInternalPlayback();
    this.#requestFrame(seconds);
    this.#video.currentTime=seconds;
  }

  #requestFrame(seconds:number){
    const request=this.#video.requestVideoFrameCallback?.bind(this.#video);
    if(!request)return;
    const target=this.#target;
    this.#frame=request((_now,metadata)=>{
      this.#frame=undefined;
      if(this.#disposed||target===undefined||this.#target!==target)return;
      if(Math.abs(metadata.mediaTime-seconds)>.15){this.#requestFrame(seconds);return}
      if(this.#pending!==undefined){this.#target=undefined;this.#issue();return}
      this.#framePresented=true;
      if(this.#seekCompleted)this.#finish();
    });
  }

  #seeked=()=>{
    if(this.#disposed||this.#target===undefined)return;
    if(this.#pending!==undefined){
      if(this.#frame!==undefined)this.#video.cancelVideoFrameCallback?.(this.#frame);
      this.#frame=undefined;
      this.#target=undefined;
      this.#issue();
      return;
    }
    this.#seekCompleted=true;
    if(!this.#video.requestVideoFrameCallback||this.#framePresented)this.#finish();
  };

  #finish(){
    const target=this.#target;
    this.#target=undefined;
    if(target!==undefined)this.#present(target);
    if(this.#pending!==undefined)this.#issue();else this.#stopInternalPlayback();
  }
}
