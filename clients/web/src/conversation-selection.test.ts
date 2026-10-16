import { describe, expect, it } from 'vitest';

import type { ConversationMessage } from './ai-conversation';
import { conversationTranscriptMarkdown } from './conversation-export';
import {
  type ConversationMessageSelection,
  conversationSelectionAfterPick,
  conversationSelectionGaps,
  conversationSelectionHint,
  type ConversationSelectionModifiers,
  conversationSelectionModifiers,
  EMPTY_CONVERSATION_SELECTION,
  selectedConversationMessagesFor,
} from './conversation-selection';

const messages: ConversationMessage[] = ['m1', 'm2', 'm3', 'm4', 'm5', 'm6'].map((id, index) => ({
  id,
  role: index % 2 ? 'assistant' : 'user',
  content: `Content ${id}`,
}));

const PLAIN: ConversationSelectionModifiers = { toggle: false, extend: false },
  TOGGLE: ConversationSelectionModifiers = { toggle: true, extend: false },
  EXTEND: ConversationSelectionModifiers = { toggle: false, extend: true },
  TOGGLE_EXTEND: ConversationSelectionModifiers = { toggle: true, extend: true };

/** Walk a pick sequence from empty, returning every intermediate selection. */
function walk(
  steps: Array<[string, ConversationSelectionModifiers]>,
  start: ConversationMessageSelection = EMPTY_CONVERSATION_SELECTION,
) {
  const states: ConversationMessageSelection[] = [];
  let current = start;
  for (const [id, modifiers] of steps) {
    current = conversationSelectionAfterPick(messages, current, id, modifiers);
    states.push(current);
  }
  return states;
}

describe('conversation message selection (HS2-WV88W3)', () => {
  it('normalizes the toggle modifier to Cmd on Apple platforms and Ctrl elsewhere', () => {
    const event = (init: Partial<MouseEvent>) => ({ metaKey: false, ctrlKey: false, shiftKey: false, ...init });
    expect(conversationSelectionModifiers(event({ metaKey: true }), true)).toEqual(TOGGLE);
    expect(conversationSelectionModifiers(event({ ctrlKey: true }), true)).toEqual(PLAIN);
    expect(conversationSelectionModifiers(event({ ctrlKey: true }), false)).toEqual(TOGGLE);
    expect(conversationSelectionModifiers(event({ metaKey: true }), false)).toEqual(PLAIN);
    expect(conversationSelectionModifiers(event({ shiftKey: true, metaKey: true }), true)).toEqual(TOGGLE_EXTEND);
    expect(conversationSelectionHint(true)).toBe(
      'Click to select. Cmd-click to add or remove, Shift-click for a range.',
    );
    expect(conversationSelectionHint(false)).toContain('Ctrl-click to add or remove');
  });

  it('plain click selects one message, replaces it, and clears when the sole selection is clicked again', () => {
    const [first, second, cleared, refilled] = walk([
      ['m2', PLAIN],
      ['m4', PLAIN],
      ['m4', PLAIN],
      ['m3', PLAIN],
    ]);
    expect(first).toEqual({ messageIds: ['m2'], anchorId: 'm2' });
    expect(second).toEqual({ messageIds: ['m4'], anchorId: 'm4' });
    expect(cleared).toEqual(EMPTY_CONVERSATION_SELECTION);
    expect(refilled).toEqual({ messageIds: ['m3'], anchorId: 'm3' });
  });

  it('toggle adds and removes single messages in transcript order and moves the anchor', () => {
    const states = walk([
      ['m5', TOGGLE],
      ['m1', TOGGLE],
      ['m3', TOGGLE],
      ['m5', TOGGLE],
    ]);
    expect(states.map((state) => state.messageIds)).toEqual([['m5'], ['m1', 'm5'], ['m1', 'm3', 'm5'], ['m1', 'm3']]);
    expect(states.map((state) => state.anchorId)).toEqual(['m5', 'm1', 'm3', 'm5']);
    // A plain click on one member of a multi-selection collapses to it rather than clearing.
    expect(conversationSelectionAfterPick(messages, states[3], 'm1', PLAIN)).toEqual({
      messageIds: ['m1'],
      anchorId: 'm1',
    });
    // Toggling the last selected message off leaves an empty selection that a pick refills.
    const emptied = walk([['m3', TOGGLE]], { messageIds: ['m3'], anchorId: 'm3' })[0];
    expect(emptied.messageIds).toEqual([]);
    expect(conversationSelectionAfterPick(messages, emptied, 'm6', TOGGLE).messageIds).toEqual(['m6']);
  });

  it('shift extends from the anchor in either direction and re-extends from the same anchor', () => {
    const [anchor, down, up] = walk([
      ['m3', PLAIN],
      ['m5', EXTEND],
      ['m1', EXTEND],
    ]);
    expect(anchor.messageIds).toEqual(['m3']);
    expect(down).toEqual({ messageIds: ['m3', 'm4', 'm5'], anchorId: 'm3' });
    expect(up).toEqual({ messageIds: ['m1', 'm2', 'm3'], anchorId: 'm3' });
    // Shift on the anchor itself narrows to the anchor without clearing it.
    expect(conversationSelectionAfterPick(messages, up, 'm3', EXTEND)).toEqual({ messageIds: ['m3'], anchorId: 'm3' });
  });

  it('shift after a toggle extends from the toggled message and replaces the rest; Cmd/Ctrl+Shift adds', () => {
    const [, toggled, replaced] = walk([
      ['m1', PLAIN],
      ['m4', TOGGLE],
      ['m6', EXTEND],
    ]);
    expect(toggled).toEqual({ messageIds: ['m1', 'm4'], anchorId: 'm4' });
    expect(replaced).toEqual({ messageIds: ['m4', 'm5', 'm6'], anchorId: 'm4' });
    const added = conversationSelectionAfterPick(messages, toggled, 'm6', TOGGLE_EXTEND);
    expect(added).toEqual({ messageIds: ['m1', 'm4', 'm5', 'm6'], anchorId: 'm4' });
  });

  it('shift without an anchor acts like a plain pick (or a toggle when combined)', () => {
    expect(conversationSelectionAfterPick(messages, EMPTY_CONVERSATION_SELECTION, 'm2', EXTEND)).toEqual({
      messageIds: ['m2'],
      anchorId: 'm2',
    });
    // A stale anchor (its message is gone) is no anchor; stale ids are dropped too.
    const stale = { messageIds: ['gone', 'm2'], anchorId: 'gone' };
    expect(conversationSelectionAfterPick(messages, stale, 'm4', EXTEND)).toEqual({
      messageIds: ['m4'],
      anchorId: 'm4',
    });
    expect(conversationSelectionAfterPick(messages, stale, 'm4', TOGGLE_EXTEND)).toEqual({
      messageIds: ['m2', 'm4'],
      anchorId: 'm4',
    });
    // Shift on the sole selected message keeps it.
    expect(conversationSelectionAfterPick(messages, { messageIds: ['m2'] }, 'm2', EXTEND).messageIds).toEqual(['m2']);
  });

  it('ignores picks of unknown messages and survives repeated, interleaved sequences', () => {
    const start = { messageIds: ['m2'], anchorId: 'm2' };
    expect(conversationSelectionAfterPick(messages, start, 'missing', TOGGLE)).toBe(start);
    const states = walk([
      ['m2', PLAIN],
      ['m2', TOGGLE],
      ['m2', TOGGLE],
      ['m6', EXTEND],
      ['m4', TOGGLE],
      ['m1', TOGGLE_EXTEND],
      ['m1', PLAIN],
      ['m1', PLAIN],
    ]);
    expect(states.map((state) => state.messageIds)).toEqual([
      ['m2'],
      [],
      ['m2'],
      ['m2', 'm3', 'm4', 'm5', 'm6'],
      ['m2', 'm3', 'm5', 'm6'],
      ['m1', 'm2', 'm3', 'm4', 'm5', 'm6'],
      ['m1'],
      [],
    ]);
  });

  it('follows the displayed sequence order rather than arrival order for ranges, reads, and gaps', () => {
    // Arrival order u1, a1, u2, a2 but the transcript shows u1, u2, a1, a2 by sequence.
    const arrived: ConversationMessage[] = [
      { id: 'u1', role: 'user', content: 'u1', sequence: 0 },
      { id: 'a1', role: 'assistant', content: 'a1', sequence: 2 },
      { id: 'u2', role: 'user', content: 'u2', sequence: 1 },
      { id: 'a2', role: 'assistant', content: 'a2', sequence: 3 },
    ];
    const range = conversationSelectionAfterPick(arrived, { messageIds: ['u1'], anchorId: 'u1' }, 'u2', EXTEND);
    expect(range.messageIds).toEqual(['u1', 'u2']);
    const gapped = selectedConversationMessagesFor(arrived, { messageIds: ['a1', 'u1'] });
    expect(gapped.map((message) => message.id)).toEqual(['u1', 'a1']);
    expect([...conversationSelectionGaps(arrived, gapped)]).toEqual([['a1', 1]]);
  });

  it('reads the selected messages in order and counts only interior gaps', () => {
    const selection = { messageIds: ['m6', 'm1', 'm2', 'm4', 'gone'] },
      selected = selectedConversationMessagesFor(messages, selection);
    expect(selected.map((message) => message.id)).toEqual(['m1', 'm2', 'm4', 'm6']);
    expect(selectedConversationMessagesFor(messages, undefined)).toEqual([]);
    expect(selectedConversationMessagesFor(messages, EMPTY_CONVERSATION_SELECTION)).toEqual([]);
    expect([...conversationSelectionGaps(messages, selected)]).toEqual([
      ['m4', 1],
      ['m6', 1],
    ]);
    // Gaps a reopened saved selection already had count toward the gap each message falls in (HS2-BZB93V).
    const prior = { m2: 3, m4: 1 };
    expect([...conversationSelectionGaps(messages, messages, prior)]).toEqual([
      ['m2', 3],
      ['m4', 1],
    ]);
    expect([
      ...conversationSelectionGaps(
        messages,
        selectedConversationMessagesFor(messages, { messageIds: ['m1', 'm5'] }),
        prior,
      ),
    ]).toEqual([['m5', 7]]);
    // A prior gap before the first selected message is leading, not interior.
    expect(
      conversationSelectionGaps(
        messages,
        selectedConversationMessagesFor(messages, { messageIds: ['m2', 'm3'] }),
        prior,
      ).size,
    ).toBe(0);
    // Leading and trailing exclusions are not gaps.
    expect(
      conversationSelectionGaps(messages, selectedConversationMessagesFor(messages, { messageIds: ['m3', 'm4'] })).size,
    ).toBe(0);
  });

  it('copies a selection with gaps as Markdown that marks each skipped run', () => {
    const selected = selectedConversationMessagesFor(messages, { messageIds: ['m1', 'm2', 'm5'] }),
      markdown = conversationTranscriptMarkdown(
        { conversationId: 'c1', tool: 'Codex' },
        selected,
        [],
        conversationSelectionGaps(messages, selected),
      );
    expect(markdown).toContain('Content m1');
    expect(markdown).toContain('Content m5');
    expect(markdown).not.toContain('Content m3');
    expect(markdown.match(/_2 messages skipped_/g)).toHaveLength(1);
    expect(markdown.indexOf('Content m2')).toBeLessThan(markdown.indexOf('_2 messages skipped_'));
    expect(markdown.indexOf('_2 messages skipped_')).toBeLessThan(markdown.indexOf('Content m5'));
    const single = selectedConversationMessagesFor(messages, { messageIds: ['m1', 'm3'] });
    expect(
      conversationTranscriptMarkdown(
        { conversationId: 'c1', tool: 'Codex' },
        single,
        [],
        conversationSelectionGaps(messages, single),
      ),
    ).toContain('_1 message skipped_');
  });
});
