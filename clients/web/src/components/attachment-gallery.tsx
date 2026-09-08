import './attachment-gallery.css';

import {ChevronLeft,ChevronRight,Minus,MoreHorizontal,Plus,X} from 'lucide';

import {isVideoAttachment} from '../attachment-references';
import {LucideIcon} from './lucide-icon';
import {Toolbar} from './toolbar';
import {ToolbarControlGroup} from './toolbar-control-group';
import {ToolbarText} from './toolbar-text';

export interface AttachmentGalleryImage {id:string;name:string;url:string;aliases?:readonly string[];ticket?:string;attachmentId?:string}
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

function GalleryButton({action,label,icon,disabled=false,className='' }:{action:string;label:string;icon:typeof X;disabled?:boolean;className?:string}) {
  return <button type="button" class={className} data-action={action} aria-label={label} title={label} disabled={disabled}><LucideIcon icon={icon} name={label.toLowerCase().replaceAll(' ','-')}/></button>;
}

export function AttachmentGallery({images,activeUrl,geometry={naturalWidth:0,naturalHeight:0,availableWidth:0,availableHeight:0},selectedScale}:{images:readonly AttachmentGalleryImage[];activeUrl:string;geometry?:AttachmentGalleryGeometry;selectedScale?:number}) {
  if(images.length===0)return null;
  const index=Math.max(0,attachmentGalleryImageIndex(images,activeUrl)),image=images[index],video=isVideoAttachment(image.name),noun=video?'video':'image',zoom=attachmentGalleryZoomModel(geometry,selectedScale);
  const imageData={'data-attachment-url':image.url,'data-attachment-name':image.name,'data-attachment-ticket':image.ticket,'data-gallery-attachment-id':image.attachmentId};
  return <div class="attachment-gallery" data-component="attachment-gallery" role="dialog" aria-modal="true" aria-label={`${video?'Video':'Image'} ${index+1} of ${images.length}: ${image.name}`}>
    <Toolbar className="attachment-gallery__toolbar" divider={false} leading={<ToolbarText className="attachment-gallery__filename" text={image.name}/>} trailing={<>
      <ToolbarControlGroup label="Media navigation" tone="dark"><GalleryButton action="previous-gallery-image" label={`Previous ${noun}`} icon={ChevronLeft} disabled={images.length<2}/><span class="attachment-gallery__count">{index+1} / {images.length}</span><GalleryButton action="next-gallery-image" label={`Next ${noun}`} icon={ChevronRight} disabled={images.length<2}/></ToolbarControlGroup>
      <ToolbarControlGroup label="Media actions" tone="dark" single><GalleryButton action="open-gallery-attachment-menu" label={`More ${noun} actions`} icon={MoreHorizontal}/></ToolbarControlGroup>
      <ToolbarControlGroup label="Close gallery" tone="dark" single><GalleryButton action="close-attachment-gallery" label={`Close ${noun} gallery`} icon={X}/></ToolbarControlGroup>
    </>}/>
    <div class="attachment-gallery__stage" data-gallery-zoom-stage="true"><div class="attachment-gallery__canvas">{video?<video {...imageData} data-gallery-media="true" src={image.url} aria-label={image.name} controls playsInline preload="metadata" style={validDimension(geometry.naturalWidth)&&validDimension(geometry.naturalHeight)?`width:${geometry.naturalWidth*zoom.scale}px;height:${geometry.naturalHeight*zoom.scale}px`:undefined}/>:<img {...imageData} data-gallery-media="true" data-gallery-image="true" src={image.url} alt={image.name} style={validDimension(geometry.naturalWidth)&&validDimension(geometry.naturalHeight)?`width:${geometry.naturalWidth*zoom.scale}px;height:${geometry.naturalHeight*zoom.scale}px`:undefined}/>}</div></div>
    <ToolbarControlGroup className="attachment-gallery__zoom" label="Media zoom" tone="dark"><button type="button" data-action="zoom-gallery-image" data-zoom-direction="out" aria-label="Zoom out" title="Zoom out" disabled={!zoom.canZoomOut}><LucideIcon icon={Minus} name="minus"/></button><button type="button" data-action="zoom-gallery-image" data-zoom-direction="in" aria-label="Zoom in" title="Zoom in" disabled={!zoom.canZoomIn}><LucideIcon icon={Plus} name="plus"/></button></ToolbarControlGroup>
  </div>;
}
