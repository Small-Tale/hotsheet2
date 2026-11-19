import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { announcerItem } from '../announcer-reel';
import { AnnouncerOverlay, type AnnouncerOverlayProps } from './announcer-overlay';

const item = announcerItem('demo', 'Demo project', 'demo-store', {
  id: 'one',
  ts: '2026-11-01T12:00:00Z',
  tool: 'codex',
  kind: 'decision',
  summary: 'codex decided: keep the synthetic sample short',
  importance: 'high',
});

const render = (props: Partial<AnnouncerOverlayProps> = {}) =>
  String(
    AnnouncerOverlay({
      open: true,
      expanded: false,
      live: false,
      speaking: false,
      audible: true,
      items: [item, { ...item, key: 'two' }],
      index: 0,
      ...props,
    }),
  );

/** The opening tag of the toolbar button whose announcer action is `name`, and its icon name. */
const toggle = (html: string, name: string) => {
  const start = html.indexOf(`data-announcer-action="${name}"`);
  const open = html.lastIndexOf('<button', start);
  const close = html.indexOf('</button>', start);
  const markup = html.slice(open, close);
  return {
    pressed: /aria-pressed="(true|false)"/.exec(markup)?.[1],
    label: /aria-label="([^"]*)"/.exec(markup)?.[1],
    icon: /data-lucide="([^"]*)"/.exec(markup)?.[1],
  };
};

describe('AnnouncerOverlay controls (HS2-EYW4H8, HS2-YF8P38)', () => {
  it('keeps playback at the toolbar start and the narration toggles at its end', () => {
    const html = render();
    const toolbar = html.indexOf('aria-label="Announcer Controls"');
    const leading = html.indexOf('kui-toolbar__leading', toolbar);
    const trailing = html.indexOf('kui-toolbar__trailing', toolbar);
    expect(toolbar).toBeGreaterThan(0);
    const order = [
      'aria-label="Playback"',
      'aria-label="Previous Activity"',
      'aria-label="Play Narration"',
      'aria-label="Next Activity"',
      'aria-label="Narration"',
      'data-announcer-action="live"',
      'data-announcer-action="audio"',
    ].map((needle) => html.indexOf(needle, toolbar));
    expect(order.every((at) => at > 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(order[0]).toBeGreaterThan(leading);
    expect(order[3]).toBeLessThan(trailing);
    expect(order[4]).toBeGreaterThan(trailing);
  });

  it('shows each toggle on and off through pressed state and the audio icon', () => {
    const off = render({ live: false, audible: false });
    const on = render({ live: true, audible: true });
    expect(toggle(off, 'live')).toEqual({ pressed: 'false', label: 'Live', icon: 'radio' });
    expect(toggle(on, 'live')).toEqual({ pressed: 'true', label: 'Live', icon: 'radio' });
    expect(toggle(off, 'audio')).toEqual({ pressed: 'false', label: 'Audio', icon: 'volume-x' });
    expect(toggle(on, 'audio')).toEqual({ pressed: 'true', label: 'Audio', icon: 'volume-2' });
  });

  it('leaves the toolbar buttons to Kerf: no app rule targets every announcer button', () => {
    const css = readFileSync(new URL('./announcer-overlay.css', import.meta.url), 'utf8');
    expect(css).not.toMatch(/\.announcer button\b/);
    expect(css).not.toMatch(/announcer__controls/);
    expect(css).not.toMatch(/aria-pressed/);
  });
});

describe('AnnouncerOverlay expanded card (HS2-7YD7S9)', () => {
  it('shows only the current item, without a history list or per-item buttons', () => {
    for (const expanded of [false, true]) {
      const html = render({ expanded, index: 1 });
      expect(html).not.toContain('<ol');
      expect(html).not.toContain('data-announcer-action="select"');
      expect(html.split('keep the synthetic sample short').length - 1).toBe(1);
    }
  });
});

describe('AnnouncerOverlay pop-out and reattach (HS2-51X25Z)', () => {
  it('offers Pop Out, and no expand or close, while attached', () => {
    for (const presentation of ['docked', 'corner'] as const) {
      for (const expanded of [false, true]) {
        const html = render({ presentation, expanded });
        expect(toggle(html, 'detach').label).toBe('Pop Out Announcer');
        expect(toggle(html, 'detach').icon).toBe('picture-in-picture-2');
        expect(html).not.toContain('data-announcer-action="expand"');
        expect(html).not.toContain('data-announcer-action="reattach"');
        expect(html).not.toContain('Close Announcer');
        expect(html).toContain('data-expanded="false"');
        expect(html).not.toContain('data-announcer-floating');
      }
    }
  });

  it('floats on a persistent Kerf surface with Expand and Reattach and no Pin or Close', () => {
    const html = render({ presentation: 'floating', surfacePresentation: 'embedded' });
    expect(html).toContain('data-component="pinnable-popover-surface"');
    expect(html).toContain('id="announcer-floating"');
    expect(html).toContain('data-announcer-floating');
    expect(html).toContain('data-persistent');
    expect(html).toContain('data-closable="false"');
    expect(html).not.toMatch(/aria-label="(?:Pin|Close)"/);
    expect(html).not.toContain('data-announcer-action="detach"');
    expect(toggle(html, 'reattach').label).toBe('Reattach Announcer');
    expect(toggle(html, 'reattach').icon).toBe('picture-in-picture');
    expect(toggle(html, 'expand')).toMatchObject({ label: 'Expand Announcer', icon: 'maximize-2' });
    expect(html).toContain('1 / 2');
    expect(html).toContain('data-presentation="floating"');
    // The surface carries the title; the section adds no second header.
    expect(html).not.toContain('class="announcer__header"');
  });

  it('expands and shrinks only while detached', () => {
    const html = render({ presentation: 'floating', surfacePresentation: 'embedded', expanded: true });
    expect(toggle(html, 'expand')).toMatchObject({ label: 'Shrink Announcer', icon: 'minimize-2' });
    expect(html).toContain('data-expanded="true"');
  });
});

describe('AnnouncerOverlay Markdown item (HS2-YJK0G9)', () => {
  const command = announcerItem('demo', 'Demo project', 'demo-store', {
    id: 'cmd',
    ts: '2026-11-01T12:00:00Z',
    tool: 'claude',
    kind: 'command',
    summary: "claude ran `cat > notes.md <<'EOF' <b>synthetic</b>` then **checked** it",
    importance: 'normal',
  });

  it('renders inline code as code and emphasis as Markdown, escaping raw HTML', () => {
    const html = render({ items: [command], index: 0 });
    expect(html).toContain('data-component="markdown-preview"');
    expect(html).toMatch(
      /<code>cat &gt; notes\.md &lt;&lt;(?:'|&#39;)EOF(?:'|&#39;) &lt;b&gt;synthetic&lt;\/b&gt;<\/code>/,
    );
    expect(html).toContain('<strong>checked</strong>');
    expect(html).not.toContain('<b>synthetic</b>');
    expect(html).not.toContain('`cat');
  });
});
