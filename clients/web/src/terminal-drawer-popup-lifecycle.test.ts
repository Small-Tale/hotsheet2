import { describe, expect, it } from 'vitest';

import { allowInterruptedDrawerPopupShow } from './interactions/terminals';

describe('terminal drawer popup reopen', () => {
  it('clears the stale active gate only on a show interrupted by hide', () => {
    const popup = { active: false },
      menu = { open: true, popup };
    allowInterruptedDrawerPopupShow(menu);
    expect(popup.active).toBe(false);

    popup.active = true;
    menu.open = false;
    allowInterruptedDrawerPopupShow(menu);
    expect(popup.active).toBe(true);

    menu.open = true;
    allowInterruptedDrawerPopupShow(menu);
    expect(popup.active).toBe(false);

    popup.active = true;
    menu.open = false;
    popup.active = false;
    menu.open = true;
    allowInterruptedDrawerPopupShow(menu);
    expect(popup.active).toBe(false);
  });
});
