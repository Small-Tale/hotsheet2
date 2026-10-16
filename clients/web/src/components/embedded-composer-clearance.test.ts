import { describe, expect, it } from 'vitest';

import {
  COMPOSER_CLEARANCE_PROPERTY,
  composerClearance,
  type ComposerClearanceEnvironment,
  wireEmbeddedComposerClearance,
} from './embedded-composer-clearance';

describe('composerClearance (HS2-W0KJVQ)', () => {
  it('reserves the distance from the composer top to the conversation bottom, rounded up', () => {
    expect(composerClearance({ top: 0, bottom: 800 }, { top: 560.2, bottom: 784 })).toBe(240);
    expect(composerClearance({ top: 0, bottom: 800 }, undefined)).toBe(0);
    expect(composerClearance({ top: 0, bottom: 800 }, { top: 900, bottom: 950 })).toBe(0);
  });
});

/** Minimal stand-ins for the conversation root, its composer form, and the wiring root. */
class FakeStyle {
  readonly values = new Map<string, string>();
  writes = 0;
  getPropertyValue(name: string) {
    return this.values.get(name) ?? '';
  }
  setProperty(name: string, value: string) {
    this.writes += 1;
    this.values.set(name, value);
  }
  removeProperty(name: string) {
    this.values.delete(name);
    return '';
  }
}
class FakeComposer {
  top = 600;
}
class FakeConversation {
  readonly style = new FakeStyle();
  isConnected = true;
  bottom = 800;
  composer: FakeComposer | undefined = new FakeComposer();
  matches(selector: string) {
    return selector === '.ai-conversation--embedded';
  }
  getBoundingClientRect() {
    return { top: 0, bottom: this.bottom };
  }
  querySelector() {
    return this.composer ? { getBoundingClientRect: () => ({ top: this.composer!.top, bottom: this.bottom }) } : null;
  }
}

function harness(initial: FakeConversation[]) {
  let conversations = initial;
  const composerNodes = new Map<FakeComposer, { getBoundingClientRect: () => { top: number; bottom: number } }>();
  // The wiring queries composers by selector; hand back one stable node per fake composer.
  for (const conversation of initial) patchComposerQuery(conversation);
  function patchComposerQuery(conversation: FakeConversation) {
    conversation.querySelector = () => {
      const composer = conversation.composer;
      if (!composer) return null;
      let node = composerNodes.get(composer);
      if (!node) {
        node = { getBoundingClientRect: () => ({ top: composer.top, bottom: conversation.bottom }) };
        composerNodes.set(composer, node);
      }
      return node;
    };
  }
  const root = {
    matches: () => false,
    querySelectorAll: () => conversations.filter((conversation) => conversation.isConnected),
  };
  let resizeCallback: (() => void) | undefined,
    mutationCallback: ((records: readonly MutationRecord[]) => void) | undefined;
  const observed = new Set<unknown>();
  const frames: (() => void)[] = [];
  let mutationsDisconnected = false;
  const environment: ComposerClearanceEnvironment = {
    createResizeObserver: (callback) => {
      resizeCallback = callback;
      return {
        observe: (target: Element) => observed.add(target),
        unobserve: (target: Element) => observed.delete(target),
        disconnect: () => {
          observed.clear();
        },
      };
    },
    createMutationObserver: (callback) => {
      mutationCallback = callback;
      return {
        observe: () => {},
        disconnect: () => {
          mutationsDisconnected = true;
        },
      };
    },
    requestFrame: (callback) => frames.push(callback),
    cancelFrame: (handle) => {
      frames[handle - 1] = () => {};
    },
  };
  const dispose = wireEmbeddedComposerClearance(root as unknown as HTMLElement, environment);
  return {
    dispose,
    observed,
    frames,
    mutationsDisconnected: () => mutationsDisconnected,
    add(conversation: FakeConversation) {
      patchComposerQuery(conversation);
      conversations = [...conversations, conversation];
    },
    resize: () => resizeCallback?.(),
    mutate: (records: Partial<MutationRecord>[]) => mutationCallback?.(records as MutationRecord[]),
    flush: () => {
      for (const frame of frames.splice(0)) frame();
    },
  };
}

const value = (conversation: FakeConversation) => conversation.style.getPropertyValue(COMPOSER_CLEARANCE_PROPERTY);

describe('wireEmbeddedComposerClearance (HS2-W0KJVQ)', () => {
  it('publishes the measured clearance at once and follows the composer as it grows and shrinks', () => {
    const chat = new FakeConversation();
    const h = harness([chat]);
    expect(value(chat)).toBe('200px');
    expect(h.observed.size).toBe(1);
    // A six-row draft under the stuck banner grows the form.
    chat.composer!.top = 420;
    h.resize();
    expect(value(chat)).toBe('380px');
    // Clearing the draft shrinks it again.
    chat.composer!.top = 680;
    h.resize();
    expect(value(chat)).toBe('120px');
  });

  it('never rewrites an unchanged value, so a style mutation cannot feed itself', () => {
    const chat = new FakeConversation();
    const h = harness([chat]);
    const writes = chat.style.writes;
    h.mutate([{ type: 'attributes', target: chat as unknown as Node }]);
    h.flush();
    h.resize();
    expect(chat.style.writes).toBe(writes);
  });

  it('restores the clearance before paint when a re-render drops it, once per frame', () => {
    const chat = new FakeConversation();
    const h = harness([chat]);
    chat.style.removeProperty(COMPOSER_CLEARANCE_PROPERTY);
    h.mutate([{ type: 'attributes', target: chat as unknown as Node }]);
    h.mutate([{ type: 'attributes', target: chat as unknown as Node }]);
    expect(h.frames).toHaveLength(1);
    h.flush();
    expect(value(chat)).toBe('200px');
  });

  it('ignores style changes elsewhere under the root', () => {
    const chat = new FakeConversation();
    const h = harness([chat]);
    h.mutate([{ type: 'attributes', target: { matches: () => false } as unknown as Node }]);
    expect(h.frames).toHaveLength(0);
  });

  it('picks up a newly rendered chat, a read-only one, and a composer that is replaced or removed', () => {
    const first = new FakeConversation();
    const h = harness([first]);
    const second = new FakeConversation();
    second.bottom = 600;
    second.composer!.top = 500;
    h.add(second);
    h.mutate([{ type: 'childList' }]);
    h.flush();
    expect(value(second)).toBe('100px');
    expect(h.observed.size).toBe(2);
    // The first chat becomes a read-only transcript: no composer, zero clearance, no observer left.
    first.composer = undefined;
    h.mutate([{ type: 'childList' }]);
    h.flush();
    expect(value(first)).toBe('0px');
    expect(h.observed.size).toBe(1);
    // The composer comes back as a new node: it is observed and measured.
    first.composer = new FakeComposer();
    first.composer.top = 700;
    h.mutate([{ type: 'childList' }]);
    h.flush();
    expect(value(first)).toBe('100px');
    expect(h.observed.size).toBe(2);
  });

  it('drops a closed chat and removes every published clearance on dispose', () => {
    const chat = new FakeConversation(),
      other = new FakeConversation();
    const h = harness([chat, other]);
    other.isConnected = false;
    h.mutate([{ type: 'childList' }]);
    h.flush();
    expect(h.observed.size).toBe(1);
    // A frame still pending at dispose never runs a sync.
    h.mutate([{ type: 'childList' }]);
    h.dispose();
    h.flush();
    expect(h.mutationsDisconnected()).toBe(true);
    expect(h.observed.size).toBe(0);
    expect(value(chat)).toBe('');
  });
});
