import {describe,expect,it} from 'vitest';

import {AttachmentContextMenu} from './attachment-context-menu';

describe('AttachmentContextMenu',()=>{
  it('uses shared MenuItems for the four attachment-row actions',()=>{
    const markup=String(AttachmentContextMenu({x:12,y:24,kind:'item'}));
    expect(markup.match(/data-component="menu-item"/g)).toHaveLength(4);
    expect(markup).toContain('aria-label="Attachment actions"');
    for(const id of ['open','download','copy-reference','remove'])expect(markup).toContain(`data-item-id="${id}"`);
    expect(markup).not.toContain('data-lucide="more-horizontal"');
    expect(markup).not.toContain('data-item-id="copy-path"');
  });

  it('preserves the gallery host actions through the same menu',()=>{
    const markup=String(AttachmentContextMenu({x:12,y:24,kind:'host',revealLabel:'Show in Finder'}));
    expect(markup.match(/data-component="menu-item"/g)).toHaveLength(5);
    expect(markup).toContain('data-item-id="copy-path"');expect(markup).toContain('Show in Finder');expect(markup).not.toContain('data-item-id="remove"');
  });
});
