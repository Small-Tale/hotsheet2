import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import {AttachmentGallery,attachmentGalleryImageIndex,attachmentGallerySelectionUrl,attachmentGalleryZoomModel,attachmentGalleryZoomStops} from './attachment-gallery';

describe('AttachmentGallery',()=>{
  const images=[{id:'a',name:'a.png',url:'/a.png'},{id:'b',name:'b.svg',url:'/b.svg'}];
  it('keeps full-screen media and its sizing wrapper square-cornered',()=>{
    const css=readFileSync(new URL('./attachment-gallery.css',import.meta.url),'utf8');
    expect(css).toMatch(/\.attachment-gallery__media-wrap \{[^}]*border-radius:0/);
    expect(css).toMatch(/\.attachment-gallery__media-wrap :is\(img,video\) \{[^}]*border-radius:0/);
  });
  it('shows the active image with accessible cyclic navigation controls',()=>{
    const markup=String(AttachmentGallery({images,activeUrl:'/b.svg'}));
    expect(markup).toContain('role="dialog"');expect(markup).toContain('Image 2 of 2: b.svg');
    expect(markup).toContain('data-action="previous-gallery-image"');expect(markup).toContain('data-action="next-gallery-image"');expect(markup).toContain('src="/b.svg"');
    expect(markup).toContain('data-action="open-gallery-attachment-menu"');expect(markup.match(/data-component="toolbar-control-group"/g)).toHaveLength(4);expect(markup.match(/data-tone="dark"/g)).toHaveLength(4);
  });
  it('selects an attachment through its Markdown by-name URL alias',()=>{
    const aliased=[images[0],{...images[1],aliases:['/tickets/HS2-DEMO/attachments/by-name/b.svg']}];
    const alias='/tickets/HS2-DEMO/attachments/by-name/b.svg';
    expect(attachmentGalleryImageIndex(aliased,alias)).toBe(1);
    expect(String(AttachmentGallery({images:aliased,activeUrl:alias}))).toContain('Image 2 of 2: b.svg');
  });
  it('uses durable attachment identity before an ambiguous shared alias',()=>{
    const ambiguous=[{...images[0],ticket:'HS2-DEMO',attachmentId:'A1',aliases:['/shared.png']},{...images[1],ticket:'HS2-DEMO',attachmentId:'A2',aliases:['/shared.png']}];
    expect(attachmentGallerySelectionUrl(ambiguous,{url:'/shared.png',attachmentId:'A2',ticket:'HS2-DEMO',name:'b.svg'})).toBe('/b.svg');
    expect(attachmentGallerySelectionUrl(ambiguous,{url:'/shared.png',ticket:'HS2-DEMO',name:'b.svg'})).toBe('/b.svg');
    expect(attachmentGallerySelectionUrl(ambiguous,{url:'/shared.png'})).toBe('/a.png');
  });
  it('renders videos paused by default with only the custom playback and volume controls',()=>{
    const markup=String(AttachmentGallery({images:[{id:'video',name:'walkthrough.mp4',url:'/walkthrough.mp4',thumbnailUrl:'/walkthrough-poster.jpg'}],activeUrl:'/walkthrough.mp4'}));
    expect(markup).toContain('Video 1 of 1: walkthrough.mp4');
    expect(markup).toContain('<video');
    expect(markup).not.toMatch(/<video[^>]*\scontrols(?:[=\s>])/);
    expect(markup).not.toContain('autoplay');
    expect(markup).toContain('data-gallery-media="true"');
    expect(markup).toContain('name="gallery-playhead"');
    expect(markup).toContain('data-action="toggle-gallery-playback"');
    expect(markup).toContain('poster="/walkthrough-poster.jpg"');
    expect(markup).toContain('data-action="toggle-gallery-volume"');
    expect(markup).toContain('data-action="toggle-gallery-muted"');
    expect(markup).toContain('name="gallery-volume"');
    expect(markup).toContain('class="attachment-gallery__volume-popup"');
    expect(markup).toContain(' hidden');
  });
  it('keeps the click-open volume popup rendered until an outside interaction closes it',()=>{
    const markup=String(AttachmentGallery({images:[{id:'video',name:'walkthrough.mp4',url:'/walkthrough.mp4'}],activeUrl:'/walkthrough.mp4',volumeOpen:true,muted:true}));
    expect(markup).toContain('aria-expanded="true"');
    expect(markup).not.toContain('class="attachment-gallery__volume-popup" role="group" aria-label="Volume controls" hidden');
    expect(markup).toContain('aria-label="Unmute video"');
    const css=readFileSync(new URL('./attachment-gallery.css',import.meta.url),'utf8');
    expect(css).not.toContain(':focus-within');
    expect(css).not.toContain(':hover,:focus-within');
  });
  it('renders selected normalized rectangles, resize handles, and draggable video range brackets in markup mode',()=>{
    const markup=String(AttachmentGallery({images:[{id:'video',name:'walkthrough.mp4',url:'/walkthrough.mp4'}],activeUrl:'/walkthrough.mp4',markup:true,drawMode:true,selectedAnnotation:'annotation-1',playheadMs:1_500,durationMs:10_000,annotations:[{id:'annotation-1',x:1000,y:2000,width:3000,height:2500,start_ms:1000,end_ms:2000,text:'Check **this**'}]}));
    expect(markup).toContain('data-action="toggle-gallery-draw"');
    expect(markup).toContain('data-action="delete-gallery-annotation"');
    expect(markup).not.toContain('data-action="set-gallery-range-start"');
    expect(markup).toContain('data-gallery-range-handle="start"');
    expect(markup).toContain('data-gallery-range-handle="end"');
    expect(markup).toContain('Annotation range start at 0:01');
    expect(markup).toContain('Annotation range end at 0:02');
    expect(markup).toContain('--annotation-start:10%;--annotation-end:20%');
    expect(markup).toContain('data-selected="true" data-has-range="true"');
    expect(markup).toContain('left:10%;top:20%;width:30%;height:25%');
    expect(markup).toContain('data-annotation-handle="se"');
    expect(markup).toContain('Check **this**');
    expect(markup).toContain('aria-label="Finish markup, 1 annotation"');
    expect(markup).toContain('attachment-gallery__annotation-count');
  });
  it('uses the same point/range controls for animated SVG annotations',()=>{
    const markup=String(AttachmentGallery({images:[{id:'svg',name:'animated.svg',url:'/animated.svg'}],activeUrl:'/animated.svg',markup:true,playheadMs:500,durationMs:2000,annotations:[{id:'point',x:100,y:100,width:1000,height:1000,start_ms:500,end_ms:500,text:''}],selectedAnnotation:'point'}));
    expect(markup).toContain('name="gallery-playhead"');
    expect(markup).toContain('data-gallery-range-handle="start"');
    expect(markup).toContain('aria-label="Annotation 1"');
  });
  it('renders wireframe-like annotation ticks that seek to their times',()=>{
    const markup=String(AttachmentGallery({images:[{id:'video',name:'walkthrough.mp4',url:'/walkthrough.mp4'}],activeUrl:'/walkthrough.mp4',durationMs:10_000,annotations:[{id:'point',x:0,y:0,width:100,height:100,start_ms:2500,end_ms:2500,text:'Point'},{id:'range',x:0,y:0,width:100,height:100,start_ms:5000,end_ms:7000,text:'Range'}]}));
    expect(markup.match(/data-action="seek-gallery-annotation"/g)).toHaveLength(2);
    expect(markup).toContain('--annotation-start:25%');
    expect(markup).toContain('--annotation-start:50%;--annotation-end:70%');
  });
  it('lays playback and zoom actions in a real footer so fit and cover measurement exclude it',()=>{
    const markup=String(AttachmentGallery({images:[{id:'video',name:'walkthrough.mp4',url:'/walkthrough.mp4'}],activeUrl:'/walkthrough.mp4'}));
    const stageEnd=markup.indexOf('</div><footer class="attachment-gallery__footer">');
    expect(stageEnd).toBeGreaterThan(markup.indexOf('data-gallery-zoom-stage="true"'));
    expect(markup.indexOf('name="gallery-playhead"')).toBeGreaterThan(stageEnd);
    const css=readFileSync(new URL('./attachment-gallery.css',import.meta.url),'utf8');
    expect(css).toContain('grid-template-rows: auto minmax(0,1fr) auto');
    expect(css).not.toContain('position:fixed; z-index:2; right:20%');
  });
  it.each([
    [{naturalWidth:2000,naturalHeight:1000,availableWidth:1000,availableHeight:800},[.5,.8,1]],
    [{naturalWidth:1000,naturalHeight:1000,availableWidth:2000,availableHeight:1500},[1,1.5,2]],
    [{naturalWidth:400,naturalHeight:800,availableWidth:600,availableHeight:400},[.5,1,1.5]],
  ] as const)('orders natural, fit, and cover zoom stops for every geometry', (geometry,expected)=>{expect(attachmentGalleryZoomStops(geometry).stops).toEqual(expected)});
  it('deduplicates equal stops and disables zoom when every scale is natural size',()=>{
    expect(attachmentGalleryZoomModel({naturalWidth:800,naturalHeight:600,availableWidth:800,availableHeight:600})).toMatchObject({stops:[1],scale:1,canZoomOut:false,canZoomIn:false});
  });
  it('falls back safely for unmeasured geometry and retains the nearest stop after resize',()=>{
    expect(attachmentGalleryZoomStops({naturalWidth:0,naturalHeight:0,availableWidth:0,availableHeight:0})).toEqual({stops:[1],fit:1});
    expect(attachmentGalleryZoomModel({naturalWidth:2000,naturalHeight:1000,availableWidth:1200,availableHeight:900},.79).scale).toBe(.9);
  });
});
