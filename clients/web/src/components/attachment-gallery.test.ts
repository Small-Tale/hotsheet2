import {describe,expect,it} from 'vitest';

import {AttachmentGallery,attachmentGalleryImageIndex,attachmentGalleryZoomModel,attachmentGalleryZoomStops} from './attachment-gallery';

describe('AttachmentGallery',()=>{
  const images=[{id:'a',name:'a.png',url:'/a.png'},{id:'b',name:'b.svg',url:'/b.svg'}];
  it('shows the active image with accessible cyclic navigation controls',()=>{
    const markup=String(AttachmentGallery({images,activeUrl:'/b.svg'}));
    expect(markup).toContain('role="dialog"');expect(markup).toContain('Image 2 of 2: b.svg');
    expect(markup).toContain('data-action="previous-gallery-image"');expect(markup).toContain('data-action="next-gallery-image"');expect(markup).toContain('src="/b.svg"');
    expect(markup).toContain('data-action="open-gallery-attachment-menu"');expect(markup.match(/data-component="toolbar-control-group"/g)).toHaveLength(4);
  });
  it('selects an attachment through its Markdown by-name URL alias',()=>{
    const aliased=[images[0],{...images[1],aliases:['/tickets/HS2-DEMO/attachments/by-name/b.svg']}];
    const alias='/tickets/HS2-DEMO/attachments/by-name/b.svg';
    expect(attachmentGalleryImageIndex(aliased,alias)).toBe(1);
    expect(String(AttachmentGallery({images:aliased,activeUrl:alias}))).toContain('Image 2 of 2: b.svg');
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
