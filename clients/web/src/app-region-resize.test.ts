import { describe, expect, it } from 'vitest';

import {
  isAppRegionId,
  loadAppRegionSize,
  normalizeAppRegionSize,
  saveAppRegionSize,
  TERMINAL_DRAWER_MIN_SIZE,
  terminalDrawerDragDecision,
  terminalDrawerMaximum,
  terminalDrawerMinimum,
} from './app-region-resize';

describe('production app region sizing', () => {
  it('recognizes only production shell regions and clamps their independent bounds', () => {
    expect(isAppRegionId('app-left-rail')).toBe(true);
    expect(isAppRegionId('app-right-rail')).toBe(true);
    expect(isAppRegionId('app-bottom-drawer')).toBe(true);
    expect(isAppRegionId('resize-demo-horizontal')).toBe(false);
    expect(normalizeAppRegionSize('app-left-rail', 100)).toBe(250);
    expect(normalizeAppRegionSize('app-left-rail', 900)).toBe(360);
    expect(normalizeAppRegionSize('app-right-rail', 100)).toBe(280);
    expect(normalizeAppRegionSize('app-right-rail', 900)).toBe(520);
  });

  it('persists drawer heights above the old fixed cap for layout-time clamping', () => {
    const storage = new Map<string, string>(),
      adapter = {
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => {
          storage.set(key, value);
        },
      };
    expect(saveAppRegionSize(adapter, 'app-bottom-drawer', 700)).toBe(700);
    expect(loadAppRegionSize(adapter, 'app-bottom-drawer')).toBe(700);
  });

  it('measures the drawer maximum from the shell bottom to the work-area top', () => {
    expect(terminalDrawerMaximum(900, 164)).toBe(736);
    expect(terminalDrawerMaximum(220, 100)).toBe(TERMINAL_DRAWER_MIN_SIZE);
  });
  it('pins the drawer at its usable minimum before a persistent collapse overshoot', () => {
    expect(terminalDrawerDragDecision(220, 520)).toEqual({ size: 228, collapse: false });
    expect(terminalDrawerDragDecision(181, 520)).toEqual({ size: 228, collapse: false });
    expect(terminalDrawerDragDecision(180, 520)).toEqual({ size: 228, collapse: true });
    expect(terminalDrawerDragDecision(700, 520)).toEqual({ size: 520, collapse: false });
  });
  it('grows the phone minimum by the home-indicator inset it pads (HS2-ZEC4QV)', () => {
    expect(terminalDrawerMinimum()).toBe(228);
    expect(terminalDrawerMinimum(34)).toBe(262);
    expect(terminalDrawerMinimum(-5)).toBe(228);
    expect(terminalDrawerMinimum(Number.NaN)).toBe(228);
    expect(terminalDrawerMaximum(300, 100, 262)).toBe(262);
    expect(terminalDrawerDragDecision(240, 520, 262)).toEqual({ size: 262, collapse: false });
    expect(terminalDrawerDragDecision(215, 520, 262)).toEqual({ size: 262, collapse: false });
    expect(terminalDrawerDragDecision(214, 520, 262)).toEqual({ size: 262, collapse: true });
  });

  it('loads defaults for missing or invalid values and persists normalized values', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => {
        values.set(key, value);
      },
    };
    expect(loadAppRegionSize(storage, 'app-left-rail')).toBe(272);
    values.set('hotsheet.layout.app-inspector.size', 'not-a-number');
    expect(loadAppRegionSize(storage, 'app-right-rail')).toBe(352);
    expect(saveAppRegionSize(storage, 'app-right-rail', 999)).toBe(520);
    expect(loadAppRegionSize(storage, 'app-right-rail')).toBe(520);
  });
});
