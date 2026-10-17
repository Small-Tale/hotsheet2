import { describe, expect, it } from 'vitest';

import { isTicketTitleExpanded, toggleTicketTitleExpanded } from './ticket-title-disclosure';

describe('mobile ticket title disclosure', () => {
  it('keeps each ticket and presentation independent across renders and repeated toggles', () => {
    const first = 'HS2-TITLE-DISCLOSURE-FIRST';
    const second = 'HS2-TITLE-DISCLOSURE-SECOND';
    expect(isTicketTitleExpanded(first, 'sidebar')).toBe(false);
    expect(toggleTicketTitleExpanded(first, 'sidebar')).toBe(true);
    expect(isTicketTitleExpanded(first, 'sidebar')).toBe(true);
    expect(isTicketTitleExpanded(first, 'reader')).toBe(false);
    expect(isTicketTitleExpanded(second, 'sidebar')).toBe(false);
    expect(toggleTicketTitleExpanded(first, 'reader')).toBe(true);
    expect(toggleTicketTitleExpanded(first, 'sidebar')).toBe(false);
    expect(isTicketTitleExpanded(first, 'reader')).toBe(true);
    expect(isTicketTitleExpanded(first, 'sidebar')).toBe(false);
    expect(toggleTicketTitleExpanded(first, 'reader')).toBe(false);
  });
});
