import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import {AttachmentGallery,attachmentGalleryImageIndex,attachmentGalleryZoomModel,attachmentGalleryZoomStops} from './attachment-gallery';

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
  it('renders videos paused by default with standard playback controls',()=>{
    const markup=String(AttachmentGallery({images:[{id:'video',name:'walkthrough.mp4',url:'/walkthrough.mp4'}],activeUrl:'/walkthrough.mp4'}));
    expect(markup).toContain('Video 1 of 1: walkthrough.mp4');
    expect(markup).toContain('<video');
    expect(markup).toContain(' controls');
    expect(markup).not.toContain('autoplay');
    expect(markup).toContain('data-gallery-media="true"');
    expect(markup).toContain('name="gallery-playhead"');
    expect(markup).toContain('data-action="toggle-gallery-playback"');
  });
  it('renders selected normalized rectangles, resize handles, and video range controls in markup mode',()=>{
    const markup=String(AttachmentGallery({images:[{id:'video',name:'walkthrough.mp4',url:'/walkthrough.mp4'}],activeUrl:'/walkthrough.mp4',markup:true,drawMode:true,selectedAnnotation:'annotation-1',playheadMs:1_500,durationMs:10_000,annotations:[{id:'annotation-1',x:1000,y:2000,width:3000,height:2500,start_ms:1000,end_ms:2000,text:'Check **this**'}]}));
    expect(markup).toContain('data-action="toggle-gallery-draw"');
    expect(markup).toContain('data-action="delete-gallery-annotation"');
    expect(markup).toContain('data-action="set-gallery-range-start"');
    expect(markup).toContain('left:10%;top:20%;width:30%;height:25%');
    expect(markup).toContain('data-annotation-handle="se"');
    expect(markup).toContain('Check **this**');
    expect(markup).toContain('aria-label="Finish markup, 1 annotation"');
    expect(markup).toContain('attachment-gallery__annotation-count');
  });
  it('uses the same point/range controls for animated SVG annotations',()=>{
    const markup=String(AttachmentGallery({images:[{id:'svg',name:'animated.svg',url:'/animated.svg'}],activeUrl:'/animated.svg',markup:true,playheadMs:500,durationMs:2000,annotations:[{id:'point',x:100,y:100,width:1000,height:1000,start_ms:500,end_ms:500,text:''}],selectedAnnotation:'point'}));
    expect(markup).toContain('name="gallery-playhead"');
    expect(markup).toContain('data-action="set-gallery-range-start"');
    expect(markup).toContain('aria-label="Annotation 1"');
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
