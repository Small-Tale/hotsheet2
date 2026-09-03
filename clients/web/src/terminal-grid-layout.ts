export const TERMINAL_GRID_HEIGHT_BREAKPOINT = 600;
export const TERMINAL_GRID_MIN_FIT = 1;
export const TERMINAL_GRID_MAX_ACROSS = 10;
export const TERMINAL_GRID_MAX_HIGH = 3;
export const TERMINAL_GRID_DEFAULT_ACROSS = 4;
export const TERMINAL_GRID_DEFAULT_HIGH = 2;
export const TERMINAL_GRID_GAP = 12;
export const TERMINAL_TILE_ASPECT = 4 / 3;

export type TerminalGridBasis = 'across' | 'high';

export interface TerminalGridLayout {
  basis: TerminalGridBasis;
  fit: number;
  max: number;
  tileWidth: number;
  tileHeight: number;
}

export function terminalGridBasis(containerHeight: number): TerminalGridBasis {
  return containerHeight > TERMINAL_GRID_HEIGHT_BREAKPOINT ? 'across' : 'high';
}

export function clampTerminalFit(value: number, basis: TerminalGridBasis): number {
  const max = basis === 'across' ? TERMINAL_GRID_MAX_ACROSS : TERMINAL_GRID_MAX_HIGH;
  const finite = Number.isFinite(value) ? Math.round(value) : TERMINAL_GRID_MIN_FIT;
  return Math.max(TERMINAL_GRID_MIN_FIT, Math.min(max, finite));
}

export function terminalGridLayout(width: number, height: number, fitAcross: number, fitHigh: number): TerminalGridLayout {
  const basis = terminalGridBasis(height);
  const fit = clampTerminalFit(basis === 'across' ? fitAcross : fitHigh, basis);
  const max = basis === 'across' ? TERMINAL_GRID_MAX_ACROSS : TERMINAL_GRID_MAX_HIGH;
  if (basis === 'across') {
    const tileWidth = Math.max(1, Math.floor((Math.max(0, width) - TERMINAL_GRID_GAP * (fit - 1)) / fit));
    return { basis, fit, max, tileWidth, tileHeight: Math.max(1, Math.floor(tileWidth / TERMINAL_TILE_ASPECT)) };
  }
  const tileHeight = Math.max(1, Math.floor((Math.max(0, height) - TERMINAL_GRID_GAP * (fit - 1)) / fit));
  return { basis, fit, max, tileWidth: Math.max(1, Math.floor(tileHeight * TERMINAL_TILE_ASPECT)), tileHeight };
}

export function adjustTerminalFit(value: number, basis: TerminalGridBasis, direction: 'in' | 'out'): number {
  return clampTerminalFit(value + (direction === 'out' ? 1 : -1), basis);
}

export function terminalPreviewText(scrollback: string, maximumLines = 18): string {
  const plain = scrollback
    // Terminal control bytes are data here: strip OSC and CSI sequences from the preview.
    // eslint-disable-next-line no-control-regex
    .replaceAll(/\u001B\][^\u0007]*(?:\u0007|\u001B\\)/g, '')
    // eslint-disable-next-line no-control-regex
    .replaceAll(/\u001B\[[0-?]*[ -/]*[@-~]/g, '')
    .replaceAll('\r', '');
  const lines = plain.split('\n');
  return lines.slice(Math.max(0, lines.length - maximumLines)).join('\n').trimEnd();
}
