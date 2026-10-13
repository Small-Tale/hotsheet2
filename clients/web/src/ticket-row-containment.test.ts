import { expect, it } from 'vitest';

import { defersContainedListScrollRestore } from './ticket-row-containment';

it('defers contained-list scroll restore only on Safari WebKit', () => {
  expect(defersContainedListScrollRestore('AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36')).toBe(false);
  expect(defersContainedListScrollRestore('AppleWebKit/605.1.15 Version/26.0 Safari/605.1.15')).toBe(true);
  expect(defersContainedListScrollRestore('Gecko/20100101 Firefox/144.0')).toBe(false);
});
