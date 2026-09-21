import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import {
  AIConversationSurface,
  AppTabMenuSurface,
  CompatibilityBannerSurface,
  ConnectionDetailsSurface,
  GallerySurface,
  NotWorkingSurface,
  ReaderLayersSurface,
  TicketContextMenuSurface,
} from './reader-overlay-surfaces';

describe('reader and overlay composition surfaces', () => {
  it('owns compatibility and connection-detail rendering', () => {
    expect(
      String(
        CompatibilityBannerSurface({
          assessment: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
        }),
      ),
    ).toBe('null');
    expect(
      String(
        CompatibilityBannerSurface({
          assessment: {
            kind: 'server_too_old',
            detail: 'Upgrade required.',
            revisionMismatch: false,
            sourceStale: false,
            canRestartServer: false,
          },
        }),
      ),
    ).toContain('Upgrade required. Safe restart is unavailable');
    expect(
      String(
        ConnectionDetailsSurface({
          assessment: {
            kind: 'unknown',
            detail: 'Unavailable.',
            revisionMismatch: false,
            sourceStale: false,
            canRestartServer: false,
          },
        }),
      ),
    ).toContain('data-component="connection-details-dialog"');
  });

  it('owns optional conversations, galleries, and context menus', () => {
    expect(String(AIConversationSurface({}))).toBe('null');
    expect(
      String(
        AIConversationSurface({
          conversation: { open: true, tool: 'Codex', messages: [], draft: '', busy: false, interruptible: false },
        }),
      ),
    ).toContain('data-component="ai-conversation"');
    expect(
      String(
        GallerySurface({
          gallery: { images: [{ id: 'proof', name: 'proof.png', url: '/proof.png' }], activeUrl: '/proof.png' },
        }),
      ),
    ).toContain('data-component="attachment-gallery"');
    expect(String(AppTabMenuSurface({ menu: { kind: 'project', id: 'demo', x: 10, y: 20 } }))).toContain(
      'data-tab-id="demo"',
    );
    expect(String(TicketContextMenuSurface({ closeDialog: undefined }))).toBe('');
  });

  it('owns reader-layer and not-working compositions', () => {
    expect(String(ReaderLayersSurface({ layers: ['first' as never, 'second' as never] }))).toBe('firstsecond');
    expect(
      String(NotWorkingSurface({ slug: 'HS2-DEMO', open: true, note: 'Needs another pass.', attachments: [] })),
    ).toContain('Not Working — HS2-DEMO');
  });

  it('keeps legacy inline render surfaces out of the application root', () => {
    const source = readFileSync(new URL('../main.tsx', import.meta.url), 'utf8');
    expect(source).not.toMatch(
      /function (Reader|ReaderLayers|VisiblePermissionPopup|AIConversationSurface|RepositoryStatusSurface|ChangeEvidenceSurface|TicketContextMenu|AttachmentMenuSurface|Gallery|CommandDialog|ConnectionDetailsSurface|CompatibilityBanner|AppTabMenuSurface|NotWorkingSurface)\(/,
    );
    for (const surface of [
      'ReaderLayersSurface',
      'AIConversationSurface',
      'RepositoryStatusSurface',
      'GallerySurface',
      'AppTabMenuSurface',
    ])
      expect(source).toContain(surface);
  });
});
