import { Terminal } from '@xterm/xterm';
import { describe, expect, it } from 'vitest';

import { terminalAnswersQueries, trackTerminalUserInput } from './terminal-query-replies';

const size = (frame: { driven_by?: string | null; answered_by?: string | null }) => ({
  pty_size: { cols: 80, rows: 24 },
  ...frame,
});

describe('terminalAnswersQueries (HS2-0GZZHV)', () => {
  it.each([
    ['no size frame yet', undefined, true],
    ['the driver, named as the answerer', size({ driven_by: 'me', answered_by: 'me' }), true],
    ['not the driver', size({ driven_by: 'other', answered_by: 'other' }), false],
    ['the named fallback of an undriven terminal', size({ driven_by: null, answered_by: 'me' }), true],
    ['not the fallback of an undriven terminal', size({ driven_by: null, answered_by: 'other' }), false],
    ['named although another viewer drives', size({ driven_by: 'preview', answered_by: 'me' }), true],
    ['nobody named', size({ driven_by: null, answered_by: null }), true],
    ['an older server: the driver', size({ driven_by: 'me' }), true],
    ['an older server: not the driver', size({ driven_by: 'other' }), false],
    ['an older server: undriven', size({ driven_by: null }), true],
    ['an older server: no driver field', size({}), true],
  ])('%s', (_, frame, expected) => {
    expect(terminalAnswersQueries('me', frame)).toBe(expected);
  });

  it('follows a handoff away and back, and the departure of the fallback', () => {
    const sequence = [
      size({ driven_by: 'me', answered_by: 'me' }),
      size({ driven_by: 'other', answered_by: 'other' }),
      size({ driven_by: 'me', answered_by: 'me' }),
      size({ driven_by: null, answered_by: 'first' }),
      size({ driven_by: null, answered_by: 'me' }),
      size({ driven_by: null, answered_by: null }),
    ];
    expect(sequence.map((frame) => terminalAnswersQueries('me', frame))).toEqual([
      true,
      false,
      true,
      false,
      true,
      true,
    ]);
  });
});

describe('trackTerminalUserInput (HS2-0GZZHV)', () => {
  // Pins the internal xterm signal this relies on: an upgrade that removes it fails here.
  it("follows the installed xterm's own user-input label", async () => {
    const terminal = new Terminal({ allowProposedApi: true }),
      tracker = trackTerminalUserInput(terminal),
      data: Array<[string, boolean]> = [];
    expect(tracker.supported).toBe(true);
    terminal.onData((value) => data.push([value, tracker.wasUserInput()]));
    terminal.input('typed', true);
    terminal.input('scripted', false);
    // The parser's replies to a cursor-position and a device-attributes query.
    await new Promise<void>((resolve) => {
      terminal.write('\u001b[6n\u001b[c', resolve);
    });
    terminal.input('again', true);
    expect(data).toEqual([
      ['typed', true],
      ['scripted', false],
      ['\u001b[1;1R', false],
      ['\u001b[?1;2c', false],
      ['again', true],
    ]);
    tracker.dispose();
    terminal.input('after', true);
    expect(data.at(-1)).toEqual(['after', false]);
    terminal.dispose();
  });

  it('treats everything as typed input when the signal is missing, so no key is ever dropped', () => {
    for (const terminal of [{}, { _core: {} }, { _core: { coreService: { onUserInput: 'no' } } }]) {
      const tracker = trackTerminalUserInput(terminal);
      expect(tracker.supported).toBe(false);
      expect([tracker.wasUserInput(), tracker.wasUserInput()]).toEqual([true, true]);
      tracker.dispose();
    }
  });
});
