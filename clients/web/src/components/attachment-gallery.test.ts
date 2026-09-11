import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import {AttachmentGallery,attachmentGalleryAnnotationTolerance,attachmentGalleryAnnotationVisible,attachmentGalleryDefaultRange,attachmentGalleryImageIndex,attachmentGalleryKeyboardAction,attachmentGallerySelectionUrl,attachmentGalleryShiftUrl,attachmentGallerySwipeDirection,attachmentGallerySwipeGesture,attachmentGalleryZoomModel,attachmentGalleryZoomStops,releaseAttachmentGalleryVideo} from './attachment-gallery';

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
  it('cycles previous and next from canonical URLs and aliases',()=>{
    const aliased=[images[0],{...images[1],aliases:['/tickets/HS2-DEMO/attachments/by-name/b.svg']}];
    expect(attachmentGalleryShiftUrl(aliased,'/b.svg',-1)).toBe('/a.png');
    expect(attachmentGalleryShiftUrl(aliased,'/a.png',1)).toBe('/b.svg');
    expect(attachmentGalleryShiftUrl(aliased,'/tickets/HS2-DEMO/attachments/by-name/b.svg',1)).toBe('/a.png');
    expect(attachmentGalleryShiftUrl(aliased,'/missing.png',1)).toBeUndefined();
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
  it('renders the decoded first video frame while paused with only the custom playback and volume controls',()=>{
    const markup=String(AttachmentGallery({images:[{id:'video',name:'walkthrough.mp4',url:'/walkthrough.mp4',thumbnailUrl:'/walkthrough-poster.jpg'}],activeUrl:'/walkthrough.mp4'}));
    expect(markup).toContain('Video 1 of 1: walkthrough.mp4');
    expect(markup).toContain('<video');
    expect(markup).not.toMatch(/<video[^>]*\scontrols(?:[=\s>])/);
    expect(markup).not.toContain('autoplay');
    expect(markup).toContain('data-gallery-media="true"');
    expect(markup).toContain('name="gallery-playhead"');
    expect(markup).toContain('data-action="toggle-gallery-playback"');
    expect(markup).toContain('src="/walkthrough.mp4"');
    expect(markup).toContain('preload="auto"');
    expect(markup).not.toContain('poster=');
    expect(markup).not.toContain('data-video-poster-url');
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
  it('renders range handles only for the currently selected timed annotation',()=>{
    const annotations=[{id:'first',x:100,y:100,width:1000,height:1000,start_ms:1000,end_ms:2000,text:'First'},{id:'second',x:200,y:200,width:1000,height:1000,start_ms:3000,end_ms:4000,text:'Second'}];
    const selected=String(AttachmentGallery({images:[{id:'video',name:'clip.mp4',url:'/clip.mp4'}],activeUrl:'/clip.mp4',markup:true,selectedAnnotation:'second',playheadMs:3000,durationMs:10_000,annotations}));
    expect(selected.match(/data-gallery-range-handle=/g)).toHaveLength(2);
    expect(selected).toContain('data-annotation-id="second" style="left:30%"');
    const deselected=String(AttachmentGallery({images:[{id:'video',name:'clip.mp4',url:'/clip.mp4'}],activeUrl:'/clip.mp4',markup:true,playheadMs:3000,durationMs:10_000,annotations}));
    expect(deselected).not.toContain('data-gallery-range-handle=');
  });
  it('uses a duration-scaled review tolerance around timed rectangles',()=>{
    const range={id:'range',x:0,y:0,width:1,height:1,start_ms:10_000,end_ms:20_000,text:''};
    expect(attachmentGalleryAnnotationTolerance(20_000)).toBe(1000);
    expect(attachmentGalleryAnnotationTolerance(200_000)).toBe(2000);
    expect(attachmentGalleryAnnotationVisible(range,9000,20_000)).toBe(true);
    expect(attachmentGalleryAnnotationVisible(range,21_000,20_000)).toBe(true);
    expect(attachmentGalleryAnnotationVisible(range,8999,20_000)).toBe(false);
    expect(attachmentGalleryAnnotationVisible(range,21_001,20_000)).toBe(false);
    expect(attachmentGalleryAnnotationVisible({...range,start_ms:15_000,end_ms:15_000},14_000,20_000)).toBe(true);
    expect(readFileSync(new URL('./attachment-gallery.css',import.meta.url),'utf8')).toContain('.attachment-gallery__annotation[hidden] { display:none; }');
  });
  it('defaults new timed annotations to five percent on each side and clamps media edges',()=>{
    expect(attachmentGalleryDefaultRange(5000,10_000)).toEqual({start_ms:4500,end_ms:5500});
    expect(attachmentGalleryDefaultRange(100,10_000)).toEqual({start_ms:0,end_ms:600});
    expect(attachmentGalleryDefaultRange(9900,10_000)).toEqual({start_ms:9400,end_ms:10_000});
    expect(attachmentGalleryDefaultRange(-100,10_000)).toEqual({start_ms:0,end_ms:500});
  });
  it('maps standard playback and frame-jogging keys with bounded repeated transitions',()=>{
    expect(attachmentGalleryKeyboardAction(' ',1500,6000)).toEqual({kind:'toggle-playback'});
    expect(attachmentGalleryKeyboardAction('k',1500,6000)).toEqual({kind:'toggle-playback'});
    expect(attachmentGalleryKeyboardAction('ArrowRight',1500,6000)).toEqual({kind:'seek',playheadMs:1533});
    expect(attachmentGalleryKeyboardAction('ArrowLeft',0,6000)).toEqual({kind:'seek',playheadMs:0});
    expect(attachmentGalleryKeyboardAction('ArrowRight',1500,6000,true)).toEqual({kind:'seek',playheadMs:2500});
    expect(attachmentGalleryKeyboardAction('j',500,6000)).toEqual({kind:'seek',playheadMs:0});
    expect(attachmentGalleryKeyboardAction('l',5500,6000)).toEqual({kind:'seek',playheadMs:6000});
    expect(attachmentGalleryKeyboardAction('Home',5500,6000)).toEqual({kind:'seek',playheadMs:0});
    expect(attachmentGalleryKeyboardAction('End',500,6000)).toEqual({kind:'seek',playheadMs:6000});
    expect(attachmentGalleryKeyboardAction('Escape',500,6000)).toBeUndefined();
    let playhead=5990;for(let index=0;index<4;index++){const action=attachmentGalleryKeyboardAction('ArrowRight',playhead,6000);if(action?.kind==='seek')playhead=action.playheadMs}expect(playhead).toBe(6000);
  });
  it('arbitrates gallery swipes away from controls, markup, and zoom panning',()=>{
    const start={pointerId:7,clientX:200,clientY:100,button:0,markup:false,stage:true,interactive:false,horizontallyScrollable:false};
    const gesture=attachmentGallerySwipeGesture(start);
    expect(gesture).toEqual({pointerId:7,startX:200,startY:100});
    expect(attachmentGallerySwipeDirection(gesture,7,140,102)).toBe(1);
    expect(attachmentGallerySwipeDirection(gesture,7,260,102)).toBe(-1);
    expect(attachmentGallerySwipeDirection(gesture,7,160,102)).toBeUndefined();
    expect(attachmentGallerySwipeDirection(gesture,7,140,170)).toBeUndefined();
    expect(attachmentGallerySwipeDirection(gesture,8,140,102)).toBeUndefined();
    for(const disabled of [{interactive:true},{markup:true},{stage:false},{horizontallyScrollable:true},{button:2}])expect(attachmentGallerySwipeGesture({...start,...disabled})).toBeUndefined();
  });
  it('releases every video resource in deterministic browser cleanup order',()=>{
    const calls:string[]=[],resource={srcObject:{} as MediaProvider|null,pause(){calls.push('pause')},removeAttribute(name:string){calls.push(`remove:${name}`)},load(){calls.push('load')}};
    releaseAttachmentGalleryVideo(resource);
    expect(calls).toEqual(['pause','remove:src','load']);
    expect(resource.srcObject).toBeNull();
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
    expect(readFileSync(new URL('./attachment-gallery.css',import.meta.url),'utf8')).toContain('.attachment-gallery__timeline-annotation[data-has-range="true"]');
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
