import { describe, expect, it } from 'vitest';

import { adjustTerminalFit, clampTerminalFit, terminalGridBasis, terminalGridLayout, terminalPreviewText } from './terminal-grid-layout';

describe('terminal grid layout', () => {
  it('switches axes only above the exact 600px boundary', () => {
    expect(terminalGridBasis(600)).toBe('high');
    expect(terminalGridBasis(601)).toBe('across');
    expect(terminalGridLayout(1200, 601, 4, 2)).toMatchObject({ basis:'across', fit:4, max:10, tileWidth:291, tileHeight:218 });
    expect(terminalGridLayout(1200, 600, 4, 2)).toMatchObject({ basis:'high', fit:2, max:3, tileWidth:392, tileHeight:294 });
  });

  it('clamps independent counts and maps plus/minus to zoom direction', () => {
    expect(clampTerminalFit(10, 'high')).toBe(3);
    expect(adjustTerminalFit(2, 'across', 'in')).toBe(1);
    expect(adjustTerminalFit(2, 'high', 'out')).toBe(3);
    expect(adjustTerminalFit(3, 'high', 'out')).toBe(3);
  });

  it('strips terminal controls and keeps only the recent preview tail', () => {
    expect(terminalPreviewText('\u001b[31mred\u001b[0m\r\none\ntwo\nthree', 2)).toBe('two\nthree');
  });
});
