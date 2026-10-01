import { describe, expect, it } from 'vitest';

import {
  automaticInputFocusAllowed,
  isMobileViewport,
  MOBILE_BREAKPOINT,
  shouldAutoOpenInspectorOnTap,
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
