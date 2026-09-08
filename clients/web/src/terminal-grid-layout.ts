export const TERMINAL_GRID_HEIGHT_BREAKPOINT = 600;
export const TERMINAL_GRID_MIN_FIT = 1;
export const TERMINAL_GRID_MAX_ACROSS = 10;
export const TERMINAL_GRID_MAX_HIGH = 3;
export const TERMINAL_GRID_DEFAULT_ACROSS = 4;
export const TERMINAL_GRID_DEFAULT_HIGH = 2;
export const TERMINAL_GRID_GAP = 12;
export const TERMINAL_GRID_CONTENT_PADDING = 12;
export const TERMINAL_VIEWPORT_ASPECT = 5 / 3;
export const TERMINAL_TILE_FRAME_INSET = 12;
export const TERMINAL_TILE_BORDER_WIDTH = 0;
export const TERMINAL_TILE_FOOTER_HEIGHT = 43.2;
export const TERMINAL_TILE_HORIZONTAL_CHROME = 2 * (TERMINAL_TILE_BORDER_WIDTH + TERMINAL_TILE_FRAME_INSET);
export const TERMINAL_TILE_VERTICAL_CHROME = 2 * (TERMINAL_TILE_BORDER_WIDTH + TERMINAL_TILE_FRAME_INSET) + TERMINAL_TILE_FOOTER_HEIGHT;
export const TERMINAL_DRAWER_MIN_TILE_HEIGHT = 160;
export const TERMINAL_DRAWER_MIN_LAYOUT_WIDTH = 240;

export type TerminalGridBasis = 'across' | 'high';

export interface TerminalGridLayout {
  basis: TerminalGridBasis;
  fit: number;
  max: number;
  tileWidth: number;
  tileHeight: number;
}

export function terminalTileHeight(tileWidth:number):number {
  const viewportWidth=Math.max(1,tileWidth-TERMINAL_TILE_HORIZONTAL_CHROME);
  return Math.max(1,Math.round(viewportWidth/TERMINAL_VIEWPORT_ASPECT+TERMINAL_TILE_VERTICAL_CHROME));
}

export function terminalTileWidth(tileHeight:number):number {
  const viewportHeight=Math.max(1,tileHeight-TERMINAL_TILE_VERTICAL_CHROME);
  return Math.max(1,Math.round(viewportHeight*TERMINAL_VIEWPORT_ASPECT+TERMINAL_TILE_HORIZONTAL_CHROME));
}

export function terminalGridContentSize(width:number,height:number,padding=TERMINAL_GRID_CONTENT_PADDING) {
  return {width:Math.max(1,width-padding*2),height:Math.max(1,height-padding*2)};
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
    return { basis, fit, max, tileWidth, tileHeight:terminalTileHeight(tileWidth) };
  }
  const tileHeight = Math.max(1, Math.floor((Math.max(0, height) - TERMINAL_GRID_GAP * (fit - 1)) / fit));
  return { basis, fit, max, tileWidth:terminalTileWidth(tileHeight), tileHeight };
}

/** The short bottom drawer deliberately has a different scale from the global
 * dashboard: level one fits one full-height row, while levels two and three mean
 * columns across the available width and may scroll vertically. */
export function terminalDrawerGridLayout(width:number,height:number,fitHigh:number):TerminalGridLayout{
  const basis:TerminalGridBasis='high',fit=clampTerminalFit(fitHigh,basis),max=TERMINAL_GRID_MAX_HIGH;
  if(fit===1){const tileHeight=Math.max(TERMINAL_DRAWER_MIN_TILE_HEIGHT,Math.floor(Math.max(0,height)));return{basis,fit,max,tileWidth:terminalTileWidth(tileHeight),tileHeight}}
  const available=Math.max(TERMINAL_DRAWER_MIN_LAYOUT_WIDTH,Math.max(0,width)),tileWidth=Math.max(1,Math.floor((available-TERMINAL_GRID_GAP*(fit-1))/fit));
  return{basis,fit,max,tileWidth,tileHeight:terminalTileHeight(tileWidth)};
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
