import { describe, expect, it } from 'vitest';

import { drawerCreateMenuPosition } from './terminal-drawer-menu-position';

describe('drawerCreateMenuPosition', () => {
  it('opens above the normal bottom rail and stays inside the right viewport edge', () => {
    expect(drawerCreateMenuPosition({ top: 800, bottom: 832, left: 1320 }, { width: 1440, height: 900 })).toEqual({
      side: 'above',
      left: -112,
      maxHeight: 784,
    });
  });

  it('opens below a tall drawer rail and limits the menu to available space', () => {
    expect(drawerCreateMenuPosition({ top: 110, bottom: 142, left: 180 }, { width: 390, height: 600 })).toEqual({
      side: 'below',
      left: -22,
      maxHeight: 442,
    });
  });

  it('allows a short viewport to scroll its menu instead of crossing either edge', () => {
    expect(drawerCreateMenuPosition({ top: 48, bottom: 80, left: 8 }, { width: 240, height: 170 })).toEqual({
      side: 'below',
      left: 0,
      maxHeight: 74,
    });
  });
});
