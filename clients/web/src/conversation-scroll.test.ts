import { describe, expect, it } from 'vitest';

import { followConversationEnd, syncConversationScroll } from './conversation-scroll';

interface FakeTranscript {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
  getBoundingClientRect: () => { top: number };
  addEventListener: (type: string, listener: () => void) => void;
  scroll: (top: number) => void;
  resize: (height: number) => void;
  move: (top: number) => void;
}

function transcript(clientHeight: number, scrollHeight: number): FakeTranscript {
  const listeners: Array<() => void> = [];
  let viewportTop = 0;
  const fake: FakeTranscript = {
    scrollTop: 0,
    clientHeight,
    scrollHeight,
    getBoundingClientRect: () => ({ top: viewportTop }),
    addEventListener: (type, listener) => {
      if (type === 'scroll') listeners.push(listener);
    },
    // Mirrors the browser: user scrolling and height clamping both dispatch `scroll`.
    scroll: (top) => {
      fake.scrollTop = Math.max(0, Math.min(top, fake.scrollHeight - fake.clientHeight));
      for (const listener of listeners) listener();
    },
    resize: (height) => {
      fake.scrollHeight = height;
      const clamped = Math.max(0, Math.min(fake.scrollTop, height - fake.clientHeight));
      if (clamped !== fake.scrollTop) fake.scroll(clamped);
    },
    move: (top) => {
      viewportTop = top;
    },
  };
  return fake;
}

function rootOf(...transcripts: FakeTranscript[]) {
  return { querySelectorAll: () => transcripts } as unknown as ParentNode;
}

describe('conversation scroll', () => {
  it('pins the first render and follows growth while the reader stays at the latest edge', () => {
    const view = transcript(300, 1_000),
      root = rootOf(view);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_000);
    view.resize(1_200);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_200);
  });

  it('does not pull a scrolled-back reader away from older messages unless opening is forced', () => {
    const view = transcript(300, 1_000),
      root = rootOf(view);
    syncConversationScroll(root);
    view.scroll(100);
    view.resize(1_200);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(100);
    syncConversationScroll(root, true);
    expect(view.scrollTop).toBe(1_200);
    view.resize(1_400);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_400);
  });

  it('keeps a scrolled-back reader in place across repeated selection rerenders', () => {
    const view = transcript(300, 1_000),
      root = rootOf(view);
    syncConversationScroll(root);
    view.scroll(150);
    for (let pass = 0; pass < 3; pass++) {
      view.resize(pass % 2 ? 1_000 : 1_040);
      syncConversationScroll(root);
      expect(view.scrollTop).toBe(150);
    }
  });

  it('keeps the same message at the same screen position as selection chrome appears and clears', () => {
    const view = transcript(300, 1_000),
      root = rootOf(view);
    syncConversationScroll(root);
    view.scroll(150);
    view.move(67);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(217);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(217);
    view.move(0);
    view.scroll(192); // Browser anchoring changed scrollTop before the render hook ran.
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(150);
    view.scroll(200);
    view.move(67);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(267);
  });

  it('still follows the latest edge when chrome changes while pinned', () => {
    const view = transcript(300, 1_000),
      root = rootOf(view);
    syncConversationScroll(root);
    view.move(67);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_000);
    view.scroll(150);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(150);
  });

  it('pins again once the reader returns to the latest edge', () => {
    const view = transcript(300, 1_000),
      root = rootOf(view);
    syncConversationScroll(root);
    view.scroll(200);
    view.scroll(690);
    view.resize(1_300);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_300);
  });

  it('stays pinned when transient content shrinks and the browser clamps the scroll', () => {
    const view = transcript(300, 1_000),
      root = rootOf(view);
    syncConversationScroll(root);
    view.resize(1_200);
    syncConversationScroll(root);
    view.resize(1_050);
    expect(view.scrollTop).toBe(750);
    view.resize(1_400);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_400);
  });

  it('tracks each mounted transcript independently', () => {
    const dialog = transcript(300, 1_000),
      drawer = transcript(200, 800),
      root = rootOf(dialog, drawer);
    syncConversationScroll(root);
    expect([dialog.scrollTop, drawer.scrollTop]).toEqual([1_000, 800]);
    dialog.scroll(50);
    drawer.resize(900);
    syncConversationScroll(root);
    expect([dialog.scrollTop, drawer.scrollTop]).toEqual([50, 900]);
  });

  it('never moves or pins a scrolled-back reader when the composer clearance changes (HS2-4J9QFX)', () => {
    const view = transcript(300, 650),
      root = rootOf(view);
    syncConversationScroll(root);
    view.scroll(150);
    // Selecting re-renders: the clearance drops (the end shortens) and the selection bar appears.
    view.resize(540);
    view.move(67);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(217);
    // The browser reports the sync's own scroll, now within the latest-edge slop of the short end.
    view.scroll(217);
    // The wiring restores the clearance and keeps only following transcripts at the end.
    view.resize(650);
    followConversationEnd(root);
    expect(view.scrollTop).toBe(217);
    view.resize(700);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(217);
  });

  it('keeps a following transcript at the end the clearance moves, and skips an unsynced one (HS2-4J9QFX)', () => {
    const view = transcript(300, 1_000),
      unsynced = transcript(300, 1_000);
    syncConversationScroll(rootOf(view));
    // A re-render drops the clearance and the sync pins the shorter end; the restore lengthens it.
    view.resize(890);
    syncConversationScroll(rootOf(view));
    expect(view.scrollTop).toBe(890);
    view.resize(1_000);
    followConversationEnd(rootOf(view, unsynced));
    expect(view.scrollTop).toBe(1_000);
    expect(unsynced.scrollTop).toBe(0);
    // The reader still scrolls back and returns to the end as before.
    view.scroll(100);
    followConversationEnd(rootOf(view));
    expect(view.scrollTop).toBe(100);
    view.scroll(700);
    view.resize(1_100);
    followConversationEnd(rootOf(view));
    expect(view.scrollTop).toBe(1_100);
  });

  it('is a no-op when no conversation is mounted', () => {
    expect(() => {
      syncConversationScroll(rootOf());
    }).not.toThrow();
  });
});
