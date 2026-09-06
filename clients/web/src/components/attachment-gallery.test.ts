import {describe,expect,it} from 'vitest';

import {AttachmentGallery,attachmentGalleryImageIndex} from './attachment-gallery';

describe('AttachmentGallery',()=>{
  const images=[{id:'a',name:'a.png',url:'/a.png'},{id:'b',name:'b.svg',url:'/b.svg'}];
  it('shows the active image with accessible cyclic navigation controls',()=>{
    const markup=String(AttachmentGallery({images,activeUrl:'/b.svg'}));
    expect(markup).toContain('role="dialog"');expect(markup).toContain('Image 2 of 2: b.svg');
    expect(markup).toContain('data-action="previous-gallery-image"');expect(markup).toContain('data-action="next-gallery-image"');expect(markup).toContain('src="/b.svg"');
  });
  it('selects an attachment through its Markdown by-name URL alias',()=>{
    const aliased=[images[0],{...images[1],aliases:['/tickets/HS2-DEMO/attachments/by-name/b.svg']}];
    const alias='/tickets/HS2-DEMO/attachments/by-name/b.svg';
    expect(attachmentGalleryImageIndex(aliased,alias)).toBe(1);
    expect(String(AttachmentGallery({images:aliased,activeUrl:alias}))).toContain('Image 2 of 2: b.svg');
  });
});
