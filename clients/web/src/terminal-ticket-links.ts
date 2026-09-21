import type { IBuffer, IDisposable, ILink, ILinkProvider, Terminal } from '@xterm/xterm';

import { ticketReferencePattern } from './ticket-link-resolution';

type TerminalBuffer = Pick<IBuffer, 'getLine' | 'length'>;

interface BufferCellPosition {
  x: number;
  y: number;
  width: number;
}

function logicalLineBounds(buffer: TerminalBuffer, lineIndex: number): { start: number; end: number } | undefined {
  if (lineIndex < 0 || lineIndex >= buffer.length || !buffer.getLine(lineIndex)) return undefined;
  let start = lineIndex;
  while (start > 0 && buffer.getLine(start)?.isWrapped) start -= 1;
  let end = lineIndex;
  while (end + 1 < buffer.length && buffer.getLine(end + 1)?.isWrapped) end += 1;
  return { start, end };
}

/** Build xterm links from the rendered buffer, where ANSI control sequences have already
 * been interpreted. `bufferLineNumber` and returned ranges use xterm's one-based contract. */
export function terminalTicketLinksForBufferLine(
  buffer: TerminalBuffer,
  columns: number,
  bufferLineNumber: number,
  activate: (reference: string) => void,
): ILink[] | undefined {
  const bounds = logicalLineBounds(buffer, bufferLineNumber - 1);
  if (!bounds) return undefined;
  let text = '';
  const positions: BufferCellPosition[] = [];
  for (let y = bounds.start; y <= bounds.end; y += 1) {
    const line = buffer.getLine(y);
    if (!line) continue;
    for (let x = 0; x < Math.min(columns, line.length); x += 1) {
      const cell = line.getCell(x);
      if (!cell || cell.getWidth() === 0) continue;
      const chars = cell.getChars() || ' ';
      text += chars;
      for (let offset = 0; offset < chars.length; offset += 1) positions.push({ x: x + 1, y: y + 1, width: cell.getWidth() });
    }
  }
  text = text.trimEnd();
  positions.length = text.length;
  const links: ILink[] = [];
  for (const match of text.matchAll(ticketReferencePattern())) {
    const startOffset = match.index;
    const endOffset = startOffset + match[0].length - 1;
    const start = positions[startOffset];
    const end = positions[endOffset];
    if (bufferLineNumber < start.y || bufferLineNumber > end.y) continue;
    links.push({
      text: match[0],
      range: { start: { x: start.x, y: start.y }, end: { x: end.x + end.width - 1, y: end.y } },
      decorations: { pointerCursor: true, underline: true },
      activate: (_event, reference) => { activate(reference); },
    });
  }
  return links.length ? links : undefined;
}

export function registerTerminalTicketLinkProvider(
  terminal: Pick<Terminal, 'buffer' | 'cols' | 'registerLinkProvider'>,
  activate: (reference: string) => void,
): IDisposable {
  const provider: ILinkProvider = {
    provideLinks: (line, callback) => {
      callback(terminalTicketLinksForBufferLine(terminal.buffer.active, terminal.cols, line, activate));
    },
  };
  return terminal.registerLinkProvider(provider);
}
