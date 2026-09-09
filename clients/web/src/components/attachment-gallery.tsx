import './attachment-gallery.css';

import {ChevronLeft,ChevronRight,Eraser,Minus,MoreHorizontal,Pause,Pencil,Play,Plus,Scan,Volume2,VolumeX,X} from 'lucide';

import type {MediaAnnotation} from '../api';
import {isVideoAttachment} from '../attachment-references';
import {LucideIcon} from './lucide-icon';
import {Toolbar} from './toolbar';
import {ToolbarControlGroup} from './toolbar-control-group';
import {ToolbarText} from './toolbar-text';

export interface AttachmentGalleryImage {id:string;name:string;url:string;thumbnailUrl?:string;aliases?:readonly string[];ticket?:string;attachmentId?:string}
export interface AttachmentGalleryGeometry {naturalWidth:number;naturalHeight:number;availableWidth:number;availableHeight:number}
export interface AttachmentGalleryZoomModel {stops:number[];index:number;fitIndex:number;scale:number;canZoomOut:boolean;canZoomIn:boolean}

const validDimension=(value:number)=>Number.isFinite(value)&&value>0;
const sameScale=(left:number,right:number)=>Math.abs(left-right)<.0001;

export function attachmentGalleryImageIndex(images:readonly AttachmentGalleryImage[],activeUrl:string):number {
  return images.findIndex(image=>image.url===activeUrl||image.aliases?.includes(activeUrl));
}

export function attachmentGalleryZoomStops(geometry:AttachmentGalleryGeometry):{stops:number[];fit:number} {
  const {naturalWidth,naturalHeight,availableWidth,availableHeight}=geometry;
  if(![naturalWidth,naturalHeight,availableWidth,availableHeight].every(validDimension))return {stops:[1],fit:1};
  const fit=Math.min(availableWidth/naturalWidth,availableHeight/naturalHeight);
  const cover=Math.max(availableWidth/naturalWidth,availableHeight/naturalHeight);
  const stops=[1,fit,cover].sort((left,right)=>left-right).filter((value,index,all)=>index===0||!sameScale(value,all[index-1]));
  return {stops,fit};
}

export function attachmentGalleryZoomModel(geometry:AttachmentGalleryGeometry,selectedScale?:number):AttachmentGalleryZoomModel {
  const {stops,fit}=attachmentGalleryZoomStops(geometry);
  const target=validDimension(selectedScale??0)?selectedScale!:fit;
  let index=0;
  for(let candidate=1;candidate<stops.length;candidate++)if(Math.abs(stops[candidate]-target)<Math.abs(stops[index]-target))index=candidate;
  const fitIndex=stops.findIndex(value=>sameScale(value,fit));
  return {stops,index,fitIndex:Math.max(0,fitIndex),scale:stops[index],canZoomOut:index>0,canZoomIn:index<stops.length-1};
}

function GalleryButton({action,label,icon,disabled=false,className='',count=0}:{action:string;label:string;icon:typeof X;disabled?:boolean;className?:string;count?:number}) {
  const accessibleLabel=count>0?`${label}, ${count} ${count===1?'annotation':'annotations'}`:label;
  return <button type="button" class={className} data-action={action} aria-label={accessibleLabel} title={accessibleLabel} disabled={disabled}><LucideIcon icon={icon} name={label.toLowerCase().replaceAll(' ','-')}/>{count>0&&<span class="attachment-gallery__annotation-count" aria-hidden="true">{count}</span>}</button>;
}

const annotationVisible=(annotation:MediaAnnotation,playheadMs:number)=>annotation.start_ms===undefined||(annotation.start_ms===annotation.end_ms?Math.abs(playheadMs-annotation.start_ms)<=250:playheadMs>=annotation.start_ms&&playheadMs<=annotation.end_ms!);
const annotationStyle=(annotation:MediaAnnotation)=>`left:${annotation.x/100}%;top:${annotation.y/100}%;width:${annotation.width/100}%;height:${annotation.height/100}%`;
const formatTime=(milliseconds:number)=>{const seconds=Math.max(0,Math.floor(milliseconds/1000));return `${Math.floor(seconds/60)}:${String(seconds%60).padStart(2,'0')}`};
const timelinePercent=(milliseconds:number|undefined,durationMs:number)=>durationMs>0?Math.max(0,Math.min(100,(milliseconds??0)*100/durationMs)):0;

export function AttachmentGallery({images,activeUrl,geometry={naturalWidth:0,naturalHeight:0,availableWidth:0,availableHeight:0},selectedScale,annotations=[],markup=false,selectedAnnotation,drawMode=false,playheadMs=0,durationMs=0,playing=false,volume=1,muted=false,volumeOpen=false,annotationEnabled=true}:{images:readonly AttachmentGalleryImage[];activeUrl:string;geometry?:AttachmentGalleryGeometry;selectedScale?:number;annotations?:readonly MediaAnnotation[];markup?:boolean;selectedAnnotation?:string;drawMode?:boolean;playheadMs?:number;durationMs?:number;playing?:boolean;volume?:number;muted?:boolean;volumeOpen?:boolean;annotationEnabled?:boolean}) {
  if(images.length===0)return null;
  const index=Math.max(0,attachmentGalleryImageIndex(images,activeUrl)),image=images[index],video=isVideoAttachment(image.name),timed=video||durationMs>0||annotations.some(annotation=>annotation.start_ms!==undefined),noun=video?'video':'image',zoom=attachmentGalleryZoomModel(geometry,selectedScale);
  const selectedTimedAnnotation=markup?annotations.find(annotation=>annotation.id===selectedAnnotation&&annotation.start_ms!==undefined):undefined;
  const imageData={'data-attachment-url':image.url,'data-attachment-name':image.name,'data-attachment-ticket':image.ticket,'data-gallery-attachment-id':image.attachmentId};
  return <div class="attachment-gallery" data-component="attachment-gallery" role="dialog" aria-modal="true" aria-label={`${video?'Video':'Image'} ${index+1} of ${images.length}: ${image.name}`}>
    <Toolbar className="attachment-gallery__toolbar" divider={false} leading={<ToolbarText className="attachment-gallery__filename" text={image.name}/>} trailing={<>
      <ToolbarControlGroup label="Media navigation" tone="dark"><GalleryButton action="previous-gallery-image" label={`Previous ${noun}`} icon={ChevronLeft} disabled={images.length<2}/><span class="attachment-gallery__count">{index+1} / {images.length}</span><GalleryButton action="next-gallery-image" label={`Next ${noun}`} icon={ChevronRight} disabled={images.length<2}/></ToolbarControlGroup>
      <ToolbarControlGroup label="Media actions" tone="dark"><GalleryButton action="toggle-gallery-markup" label={markup?'Finish markup':'Annotate media'} icon={Pencil} disabled={!annotationEnabled} className={markup?'attachment-gallery__pressed':''} count={annotations.length}/><GalleryButton action="open-gallery-attachment-menu" label={`More ${noun} actions`} icon={MoreHorizontal}/></ToolbarControlGroup>
      <ToolbarControlGroup label="Close gallery" tone="dark" single><GalleryButton action="close-attachment-gallery" label={`Close ${noun} gallery`} icon={X}/></ToolbarControlGroup>
    </>}/>
    <div class="attachment-gallery__stage" data-gallery-zoom-stage="true"><div class="attachment-gallery__canvas"><div class="attachment-gallery__media-wrap" data-gallery-annotation-surface="true" data-draw-mode={String(drawMode)} style={validDimension(geometry.naturalWidth)&&validDimension(geometry.naturalHeight)?`width:${geometry.naturalWidth*zoom.scale}px;height:${geometry.naturalHeight*zoom.scale}px`:undefined}>{video?<video {...imageData} data-gallery-media="true" data-video-poster-url={image.thumbnailUrl} data-video-source-url={image.thumbnailUrl?image.url:undefined} src={image.url} poster={image.thumbnailUrl} aria-label={image.name} playsInline preload="metadata"/>:<img {...imageData} data-gallery-media="true" data-gallery-image="true" src={image.url} alt={image.name}/>} {markup&&<div class="attachment-gallery__annotations">{annotations.map((annotation,annotationIndex)=><button type="button" class="attachment-gallery__annotation" data-action="select-gallery-annotation" data-annotation-id={annotation.id} data-annotation-start={annotation.start_ms} data-annotation-end={annotation.end_ms} data-selected={String(annotation.id===selectedAnnotation)} hidden={!annotationVisible(annotation,playheadMs)} style={annotationStyle(annotation)} aria-label={`Annotation ${annotationIndex+1}${annotation.text?`: ${annotation.text}`:''}`}><span class="attachment-gallery__annotation-label" data-action="edit-gallery-annotation" data-annotation-id={annotation.id}>{annotation.text||String(annotationIndex+1)}</span>{annotation.id===selectedAnnotation&&['nw','n','ne','e','se','s','sw','w'].map(handle=><i data-annotation-handle={handle}/>)}</button>)}</div>}</div></div></div>
    <footer class="attachment-gallery__footer">
      {timed&&<div class="attachment-gallery__timeline"><button class="attachment-gallery__playback" type="button" data-action="toggle-gallery-playback" aria-label={playing?'Pause':'Play'}><LucideIcon icon={playing?Pause:Play} name={playing?'pause':'play'}/></button><span data-gallery-current-time="true">{formatTime(playheadMs)}</span><div class="attachment-gallery__timeline-track">{annotations.filter(annotation=>annotation.start_ms!==undefined).map((annotation,annotationIndex)=>{const start=timelinePercent(annotation.start_ms,durationMs),end=timelinePercent(annotation.end_ms??annotation.start_ms,durationMs),hasRange=end>start;return <button type="button" class="attachment-gallery__timeline-annotation" data-action="seek-gallery-annotation" data-annotation-id={annotation.id} data-annotation-time={annotation.start_ms} data-selected={String(annotation.id===selectedAnnotation)} data-has-range={String(hasRange)} style={`--annotation-start:${start}%;--annotation-end:${end}%`} aria-label={`Annotation ${annotationIndex+1} at ${formatTime(annotation.start_ms??0)}${annotation.text?`: ${annotation.text}`:''}`}/>})}<input type="range" name="gallery-playhead" min="0" max={String(Math.max(1,durationMs))} value={String(playheadMs)} aria-label="Video position"/>{selectedTimedAnnotation&&<><button type="button" class="attachment-gallery__range-handle attachment-gallery__range-handle--start" data-gallery-range-handle="start" data-annotation-id={selectedTimedAnnotation.id} style={`left:${timelinePercent(selectedTimedAnnotation.start_ms,durationMs)}%`} aria-label={`Annotation range start at ${formatTime(selectedTimedAnnotation.start_ms??0)}`}>[</button><button type="button" class="attachment-gallery__range-handle attachment-gallery__range-handle--end" data-gallery-range-handle="end" data-annotation-id={selectedTimedAnnotation.id} style={`left:${timelinePercent(selectedTimedAnnotation.end_ms??selectedTimedAnnotation.start_ms,durationMs)}%`} aria-label={`Annotation range end at ${formatTime(selectedTimedAnnotation.end_ms??selectedTimedAnnotation.start_ms??0)}`}>]</button></>}</div><span>{formatTime(durationMs)}</span>{video&&<div class="attachment-gallery__volume"><button type="button" data-action="toggle-gallery-volume" aria-label="Volume controls" aria-expanded={String(volumeOpen)}><LucideIcon icon={muted||volume===0?VolumeX:Volume2} name={muted||volume===0?'volume-x':'volume-2'}/></button><div class="attachment-gallery__volume-popup" role="group" aria-label="Volume controls" hidden={!volumeOpen}><span>Volume</span><input type="range" name="gallery-volume" min="0" max="1" step="0.05" value={String(volume)} aria-label="Video volume"/><button type="button" data-action="toggle-gallery-muted" aria-label={muted||volume===0?'Unmute video':'Mute video'}><LucideIcon icon={muted||volume===0?VolumeX:Volume2} name={muted||volume===0?'volume-x':'volume-2'}/><span>{muted||volume===0?'Unmute':'Mute'}</span></button></div></div>}</div>}
      <div class="attachment-gallery__footer-actions">{markup&&<ToolbarControlGroup className="attachment-gallery__markup" label="Media markup" tone="dark"><GalleryButton action="toggle-gallery-draw" label="Add rectangle" icon={Scan} className={drawMode?'attachment-gallery__pressed':''}/><GalleryButton action="delete-gallery-annotation" label="Erase selected annotation" icon={Eraser} disabled={!selectedAnnotation}/></ToolbarControlGroup>}<ToolbarControlGroup className="attachment-gallery__zoom" label="Media zoom" tone="dark"><button type="button" data-action="zoom-gallery-image" data-zoom-direction="out" aria-label="Zoom out" title="Zoom out" disabled={!zoom.canZoomOut}><LucideIcon icon={Minus} name="minus"/></button><button type="button" data-action="zoom-gallery-image" data-zoom-direction="in" aria-label="Zoom in" title="Zoom in" disabled={!zoom.canZoomIn}><LucideIcon icon={Plus} name="plus"/></button></ToolbarControlGroup></div>
    </footer>
  </div>;
}
