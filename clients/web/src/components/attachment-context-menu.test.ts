import { describe, expect, it } from 'vitest';

import { ATTACHMENT_CONTEXT_MENU_HEIGHT, AttachmentContextMenu } from './attachment-context-menu';

describe('AttachmentContextMenu', () => {
  it('uses Kerf menu items for attachment-row actions including host-native reveal', () => {
    const markup = String(AttachmentContextMenu({ x: 12, y: 24, kind: 'item', revealLabel: 'Show in Finder' }));
    expect(markup.match(/class="kui-popup-menu__item"/g)).toHaveLength(6);
    expect(markup).toContain('aria-label="Attachment Actions"');
    for (const id of ['open', 'download', 'copy-reference', 'rename', 'reveal', 'remove'])
      expect(markup).toContain(`data-item-id="${id}"`);
    expect(markup).toContain('Show in Finder');
    expect(markup).not.toContain('data-lucide="more-horizontal"');
    expect(markup).not.toContain('data-item-id="copy-path"');
    expect(ATTACHMENT_CONTEXT_MENU_HEIGHT).toBe(274);
  });

  it('offers a delete-only provider only Download, Copy reference, and Remove (HS2-51ADSG)', () => {
    const markup = String(AttachmentContextMenu({ x: 12, y: 24, kind: 'remote' }));
    expect(markup.match(/class="kui-popup-menu__item"/g)).toHaveLength(3);
    for (const id of ['download', 'copy-reference', 'remove']) expect(markup).toContain(`data-item-id="${id}"`);
    for (const id of ['open', 'rename', 'reveal', 'copy-path']) expect(markup).not.toContain(`data-item-id="${id}"`);
    expect(markup).toContain('data-kind="remote"');
    expect(markup).toContain('data-lucide="trash-2"');
  });

  it('preserves the gallery host actions through the same menu', () => {
    const markup = String(AttachmentContextMenu({ x: 12, y: 24, kind: 'host', revealLabel: 'Show in Finder' }));
    expect(markup.match(/class="kui-popup-menu__item"/g)).toHaveLength(6);
    expect(markup).toContain('data-item-id="copy-path"');
    expect(markup).toContain('Show in Finder');
    expect(markup).toContain('data-item-id="remove"');
  });
});
