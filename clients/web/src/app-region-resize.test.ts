import { describe, expect, it } from 'vitest';

import { isAppRegionId, loadAppRegionSize, normalizeAppRegionSize, saveAppRegionSize, terminalDrawerMaximum } from './app-region-resize';

describe('production app region sizing', () => {
  it('recognizes only production shell regions and clamps their independent bounds', () => {
    expect(isAppRegionId('app-sidebar')).toBe(true);
    expect(isAppRegionId('app-inspector')).toBe(true);
    expect(isAppRegionId('app-terminal-drawer')).toBe(true);
    expect(isAppRegionId('resize-demo-horizontal')).toBe(false);
    expect(normalizeAppRegionSize('app-sidebar', 100)).toBe(250);
    expect(normalizeAppRegionSize('app-sidebar', 900)).toBe(360);
    expect(normalizeAppRegionSize('app-inspector', 100)).toBe(280);
    expect(normalizeAppRegionSize('app-inspector', 900)).toBe(520);
  });

  it('persists drawer heights above the old fixed cap for layout-time clamping',()=>{const storage=new Map<string,string>(),adapter={getItem:(key:string)=>storage.get(key)??null,setItem:(key:string,value:string)=>{storage.set(key,value)}};expect(saveAppRegionSize(adapter,'app-terminal-drawer',700)).toBe(700);expect(loadAppRegionSize(adapter,'app-terminal-drawer')).toBe(700)});

  it('measures the drawer maximum from the shell bottom to the work-area top',()=>{expect(terminalDrawerMaximum(900,164)).toBe(736);expect(terminalDrawerMaximum(220,100)).toBe(180)});

  it('loads defaults for missing or invalid values and persists normalized values', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };
    expect(loadAppRegionSize(storage, 'app-sidebar')).toBe(272);
    values.set('hotsheet.layout.app-inspector.size', 'not-a-number');
    expect(loadAppRegionSize(storage, 'app-inspector')).toBe(352);
    expect(saveAppRegionSize(storage, 'app-inspector', 999)).toBe(520);
    expect(loadAppRegionSize(storage, 'app-inspector')).toBe(520);
  });
});
