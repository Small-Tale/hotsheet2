import './attachment-gallery.css';

import {ChevronLeft,ChevronRight,X} from 'lucide';

import {LucideIcon} from './lucide-icon';

export interface AttachmentGalleryImage {id:string;name:string;url:string}

export function AttachmentGallery({images,activeUrl}:{images:readonly AttachmentGalleryImage[];activeUrl:string}) {
  if(images.length===0)return null;
  const index=Math.max(0,images.findIndex(image=>image.url===activeUrl)),image=images[index];
  return <div class="attachment-gallery" data-component="attachment-gallery" role="dialog" aria-modal="true" aria-label={`Image ${index+1} of ${images.length}: ${image.name}`}>
    <header><span>{image.name}</span><span>{index+1} / {images.length}</span><button type="button" data-action="close-attachment-gallery" aria-label="Close image gallery"><LucideIcon icon={X} name="x"/></button></header>
    <button type="button" class="attachment-gallery__previous" data-action="previous-gallery-image" aria-label="Previous image" disabled={images.length<2}><LucideIcon icon={ChevronLeft} name="chevron-left"/></button>
    <img src={image.url} alt={image.name}/>
    <button type="button" class="attachment-gallery__next" data-action="next-gallery-image" aria-label="Next image" disabled={images.length<2}><LucideIcon icon={ChevronRight} name="chevron-right"/></button>
  </div>;
}
