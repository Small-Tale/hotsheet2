import { expect, it } from 'vitest';

import { defersContainedListScrollRestore, supportsTicketRowContainment } from './ticket-row-containment';

it('enables offscreen row containment on Blink and WebKit with a WebKit scroll fallback', () => {
  expect(supportsTicketRowContainment('AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36')).toBe(true);
  expect(supportsTicketRowContainment('AppleWebKit/537.36 HeadlessChrome/140.0.0.0 Safari/537.36')).toBe(true);
  expect(supportsTicketRowContainment('AppleWebKit/537.36 Edg/140.0.0.0')).toBe(true);
  expect(supportsTicketRowContainment('AppleWebKit/605.1.15 Version/26.0 Safari/605.1.15')).toBe(true);
  expect(supportsTicketRowContainment('Gecko/20100101 Firefox/144.0')).toBe(false);
  expect(defersContainedListScrollRestore('AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36')).toBe(false);
  expect(defersContainedListScrollRestore('AppleWebKit/605.1.15 Version/26.0 Safari/605.1.15')).toBe(true);
  expect(defersContainedListScrollRestore('Gecko/20100101 Firefox/144.0')).toBe(false);
});
