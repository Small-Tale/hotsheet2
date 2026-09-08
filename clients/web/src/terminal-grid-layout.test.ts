import { describe, expect, it } from 'vitest';

import { adjustTerminalFit, clampTerminalFit, TERMINAL_TILE_FOOTER_HEIGHT, TERMINAL_TILE_FRAME_INSET, TERMINAL_TILE_VERTICAL_CHROME, TERMINAL_VIEWPORT_ASPECT, terminalDrawerGridLayout, terminalGridBasis, terminalGridContentSize, terminalGridLayout, terminalPreviewText, terminalTileHeight, terminalTileWidth } from './terminal-grid-layout';

describe('terminal grid layout', () => {
  it('switches axes only above the exact 600px boundary', () => {
    expect(terminalGridBasis(600)).toBe('high');
    expect(terminalGridBasis(601)).toBe('across');
    expect(terminalGridLayout(1200, 601, 4, 2)).toMatchObject({ basis:'across', fit:4, max:10, tileWidth:291, tileHeight:227 });
    expect(terminalGridLayout(1200, 600, 4, 2)).toMatchObject({ basis:'high', fit:2, max:3, tileWidth:402, tileHeight:294 });
  });

  it('clamps independent counts and maps plus/minus to zoom direction', () => {
    expect(clampTerminalFit(10, 'high')).toBe(3);
    expect(adjustTerminalFit(2, 'across', 'in')).toBe(1);
    expect(adjustTerminalFit(2, 'high', 'out')).toBe(3);
    expect(adjustTerminalFit(3, 'high', 'out')).toBe(3);
  });

  it('removes both container insets before fitting drawer tiles',()=>{
    expect(terminalGridContentSize(900,320)).toEqual({width:876,height:296});
    expect(terminalGridLayout(876,296,2,1)).toMatchObject({basis:'high',fit:1,tileWidth:405,tileHeight:296});
  });

  it('fits drawer level one by height but levels two and three by width',()=>{
    expect(terminalDrawerGridLayout(876,296,1)).toMatchObject({basis:'high',fit:1,tileWidth:405,tileHeight:296});
    expect(terminalDrawerGridLayout(876,296,2)).toMatchObject({basis:'high',fit:2,tileWidth:432,tileHeight:312});
    expect(terminalDrawerGridLayout(876,296,3)).toMatchObject({basis:'high',fit:3,tileWidth:284,tileHeight:223});
  });

  it('keeps a 160px level-one tile and treats narrower layout widths as 240px',()=>{
    expect(terminalDrawerGridLayout(180,100,1)).toMatchObject({tileWidth:179,tileHeight:160});
    expect(terminalDrawerGridLayout(180,300,2)).toMatchObject({tileWidth:114,tileHeight:121});
  });

  it('adds token-mirrored borderless card chrome outside the five-by-three terminal viewport',()=>{
    expect(TERMINAL_VIEWPORT_ASPECT).toBe(5/3);
    expect(TERMINAL_TILE_FRAME_INSET).toBe(12);
    expect(TERMINAL_TILE_FOOTER_HEIGHT).toBe(43.2);
    expect(TERMINAL_TILE_VERTICAL_CHROME).toBe(67.2);
    expect(terminalTileHeight(818)).toBe(544);
    expect(terminalTileWidth(541)).toBe(814);
  });

  it('strips terminal controls and keeps only the recent preview tail', () => {
    expect(terminalPreviewText('\u001b[31mred\u001b[0m\r\none\ntwo\nthree', 2)).toBe('two\nthree');
  });
});
