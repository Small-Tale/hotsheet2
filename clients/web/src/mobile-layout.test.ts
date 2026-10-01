import { describe, expect, it } from 'vitest';

import {
  automaticInputFocusAllowed,
  isMobileViewport,
  isSidePanelPortal,
  MOBILE_BREAKPOINT,
  shouldAutoOpenInspectorOnTap,
  SIDE_PANEL_PORTAL_SELECTOR,
} from './mobile-layout';

describe('mobile layout', () => {
  it('treats widths below the breakpoint as mobile', () => {
    expect(isMobileViewport(390)).toBe(true);
    expect(isMobileViewport(1023)).toBe(true);
    expect(isMobileViewport(MOBILE_BREAKPOINT)).toBe(false);
    expect(isMobileViewport(1280)).toBe(false);
  });

  it('auto-opens the inspector only on a plain mobile workspace tap (HS2-N7RPFP)', () => {
    const base = { mobile: true, rail: false, shiftKey: false, metaKey: false, ctrlKey: false };
    expect(shouldAutoOpenInspectorOnTap(base)).toBe(true);
    expect(shouldAutoOpenInspectorOnTap({ ...base, mobile: false })).toBe(false); // desktop keeps the side inspector
    expect(shouldAutoOpenInspectorOnTap({ ...base, rail: true })).toBe(false); // terminal ticket rail
    expect(shouldAutoOpenInspectorOnTap({ ...base, shiftKey: true })).toBe(false); // range multi-select
    expect(shouldAutoOpenInspectorOnTap({ ...base, metaKey: true })).toBe(false); // toggle multi-select
    expect(shouldAutoOpenInspectorOnTap({ ...base, ctrlKey: true })).toBe(false);
  });
});

describe('automaticInputFocusAllowed (HS2-YD7RZ7)', () => {
  it('lets only larger layouts focus inputs automatically', () => {
    expect(automaticInputFocusAllowed(390)).toBe(false);
    expect(automaticInputFocusAllowed(MOBILE_BREAKPOINT - 1)).toBe(false);
    expect(automaticInputFocusAllowed(MOBILE_BREAKPOINT)).toBe(true);
    expect(automaticInputFocusAllowed(1440)).toBe(true);
  });
});

describe('isSidePanelPortal (HS2-5APX20)', () => {
  // A minimal element stand-in: it matches a selector list when one of its own selectors is listed.
  const element = (...own: string[]) =>
    ({ matches: (selector: string) => selector.split(', ').some((part) => own.includes(part)) }) as unknown as Node;

  it('keeps a phone side panel open for the menus and dialogs it launches', () => {
    for (const own of [
      'wa-dialog',
      'wa-drawer',
      '[role="dialog"]',
      '[role="alertdialog"]',
      '[role="menu"]',
      '[role="listbox"]',
      '[data-component="saved-view-context-menu"]',
    ]) {
      expect(SIDE_PANEL_PORTAL_SELECTOR.split(', ')).toContain(own);
      expect(isSidePanelPortal(element(own))).toBe(true);
    }
  });

  it('still lets the backdrop, the main column, and non-elements close the panel', () => {
    expect(isSidePanelPortal(element('.app-shell__scrim'))).toBe(false);
    expect(isSidePanelPortal(element('.app-shell__main', '[role="region"]'))).toBe(false);
    expect(isSidePanelPortal({ nodeType: 3 } as unknown as Node)).toBe(false);
    expect(isSidePanelPortal({} as Node)).toBe(false);
  });
});
