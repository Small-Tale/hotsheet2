import { beforeEach, describe, expect, it, vi } from 'vitest';

import { bootUxDemo, prepareUxDemoDocument, UX_DEMO_TITLE } from './ux-demo-entry';

describe('production /ux-demo entry (HS2-ZF70Y9)', () => {
  beforeEach(() => {
    document.head.innerHTML = '<title>Hot Sheet</title>';
    document.body.innerHTML = '<main id="app" class="kui-app-root" data-viewport="locked"></main>';
  });

  it('replaces the app root with the catalog root the development page renders', () => {
    const root = prepareUxDemoDocument(document);
    expect(document.getElementById('app')).toBeNull();
    expect(root.id).toBe('ux-demo');
    expect(root.tagName).toBe('DIV');
    expect(root.className).toBe('kui-app-root');
    expect(root.hasAttribute('data-viewport')).toBe(false);
    expect(document.body.children).toHaveLength(1);
    expect(document.title).toBe(UX_DEMO_TITLE);
    expect(document.querySelector('meta[name="robots"]')?.getAttribute('content')).toBe('noindex,nofollow');
  });

  it('is idempotent and adds a root to a document without an app root', () => {
    const first = prepareUxDemoDocument(document);
    expect(prepareUxDemoDocument(document)).toBe(first);
    expect(document.querySelectorAll('#ux-demo')).toHaveLength(1);
    expect(document.querySelectorAll('meta[name="robots"]')).toHaveLength(1);
    document.body.innerHTML = '';
    expect(prepareUxDemoDocument(document).parentElement).toBe(document.body);
  });

  it('loads the catalog only after its root exists', async () => {
    const load = vi.fn(() => {
      expect(document.getElementById('ux-demo')).not.toBeNull();
      return Promise.resolve();
    });
    await bootUxDemo(document, load);
    expect(load).toHaveBeenCalledOnce();
  });
});
