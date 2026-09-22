import type { IBuffer, IBufferCell, IBufferLine, ILinkProvider } from '@xterm/xterm';
import { describe, expect, it, vi } from 'vitest';

import { registerTerminalTicketLinkProvider, terminalTicketLinksForBufferLine } from './terminal-ticket-links';

function buffer(
  rows: Array<{ text: string; wrapped?: boolean }>,
  columns: number,
): Pick<IBuffer, 'getLine' | 'length'> {
  const lines = rows.map(
    ({ text, wrapped }) =>
      ({
        isWrapped: Boolean(wrapped),
        length: columns,
        getCell: (x: number) =>
          ({
            getChars: () => text[x] ?? '',
            getWidth: () => 1,
          }) as IBufferCell,
      }) as IBufferLine,
  );
  return { length: lines.length, getLine: (index) => lines[index] };
}

describe('terminal ticket links', () => {
  it('finds current, legacy, and project-qualified references among punctuation and ANSI-rendered cells', () => {
    // xterm has already consumed ANSI escape sequences before exposing styled cells here.
    const active = buffer([{ text: 'styled: (HS2-TRQDH2), HS-1; @other/HS2-ABC123.' }], 64);
    const links = terminalTicketLinksForBufferLine(active, 64, 1, vi.fn());
    expect(links?.map((link) => link.text)).toEqual(['HS2-TRQDH2', 'HS-1', '@other/HS2-ABC123']);
    expect(links?.map((link) => link.range)).toEqual([
      { start: { x: 10, y: 1 }, end: { x: 19, y: 1 } },
      { start: { x: 23, y: 1 }, end: { x: 26, y: 1 } },
      { start: { x: 29, y: 1 }, end: { x: 45, y: 1 } },
    ]);
  });

  it('maps a reference across wrapped buffer rows and returns it for either row', () => {
    const active = buffer([{ text: 'prefix HS2-' }, { text: 'WRAPPED end', wrapped: true }], 11);
    for (const line of [1, 2]) {
      const links = terminalTicketLinksForBufferLine(active, 11, line, vi.fn());
      expect(links?.[0]).toMatchObject({ text: 'HS2-WRAPPED', range: { start: { x: 8, y: 1 }, end: { x: 7, y: 2 } } });
    }
  });

  it('rejects invalid near-matches and keeps distinct references on one row', () => {
    const active = buffer([{ text: 'xHS2-AB HS2-A HS--10 HS2-OK and HS2-NEXT' }], 48);
    expect(terminalTicketLinksForBufferLine(active, 48, 1, vi.fn())?.map((link) => link.text)).toEqual([
      'HS2-OK',
      'HS2-NEXT',
    ]);
  });

  it('activates exact references, ignores selection drags, and resumes after selection clears', () => {
    const active = buffer([{ text: 'open HS2-ABC123' }], 24);
    const activate = vi.fn(),
      dispose = vi.fn();
    let provider: ILinkProvider | undefined;
    let selected = false;
    const registration = registerTerminalTicketLinkProvider(
      {
        buffer: { active } as never,
        cols: 24,
        hasSelection: () => selected,
        registerLinkProvider: (next) => {
          provider = next;
          return { dispose };
        },
      },
      activate,
    );
    let links;
    provider?.provideLinks(1, (value) => {
      links = value;
    });
    expect(links).toHaveLength(1);
    (links as ReturnType<typeof terminalTicketLinksForBufferLine>)?.[0]?.activate({} as MouseEvent, 'HS2-ABC123');
    expect(activate).toHaveBeenCalledWith('HS2-ABC123');
    selected = true;
    (links as ReturnType<typeof terminalTicketLinksForBufferLine>)?.[0]?.activate({} as MouseEvent, 'HS2-ABC123');
    expect(activate).toHaveBeenCalledTimes(1);
    selected = false;
    (links as ReturnType<typeof terminalTicketLinksForBufferLine>)?.[0]?.activate({} as MouseEvent, 'HS2-ABC123');
    expect(activate).toHaveBeenCalledTimes(2);
    registration.dispose();
    expect(dispose).toHaveBeenCalledOnce();
  });
});
