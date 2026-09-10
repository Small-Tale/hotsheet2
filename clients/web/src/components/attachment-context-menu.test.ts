import {describe,expect,it} from 'vitest';

import {ATTACHMENT_CONTEXT_MENU_HEIGHT,AttachmentContextMenu} from './attachment-context-menu';

describe('AttachmentContextMenu',()=>{
  it('uses shared MenuItems for attachment-row actions including host-native reveal',()=>{
    const markup=String(AttachmentContextMenu({x:12,y:24,kind:'item',revealLabel:'Show in Finder'}));
    expect(markup.match(/data-component="menu-item"/g)).toHaveLength(6);
    expect(markup).toContain('aria-label="Attachment actions"');
    for(const id of ['open','download','copy-reference','rename','reveal','remove'])expect(markup).toContain(`data-item-id="${id}"`);
    expect(markup).toContain('Show in Finder');
    expect(markup).not.toContain('data-lucide="more-horizontal"');
    expect(markup).not.toContain('data-item-id="copy-path"');
    expect(ATTACHMENT_CONTEXT_MENU_HEIGHT).toBe(274);
  });

  it('preserves the gallery host actions through the same menu',()=>{
    const markup=String(AttachmentContextMenu({x:12,y:24,kind:'host',revealLabel:'Show in Finder'}));
    expect(markup.match(/data-component="menu-item"/g)).toHaveLength(6);
    expect(markup).toContain('data-item-id="copy-path"');expect(markup).toContain('Show in Finder');expect(markup).toContain('data-item-id="remove"');
  });
});
