import { afterEach, describe, expect, it, vi } from 'vitest';

import { galleryKeyTargetsTextField } from './shell-and-global';

class FieldElement {
  constructor(readonly selector: string) {}
  matches(selector: string) {
    return selector.split(', ').includes(this.selector);
  }
}

afterEach(() => vi.unstubAllGlobals());

describe('gallery keyboard ownership', () => {
  it('leaves note editing and shadow-hosted text fields in control of their keys', () => {
    vi.stubGlobal('HTMLElement', FieldElement);
    const keyFrom = (...path: FieldElement[]) => ({ composedPath: () => path }) as unknown as Event;

    expect(galleryKeyTargetsTextField(keyFrom(new FieldElement('textarea')))).toBe(true);
    expect(
      galleryKeyTargetsTextField(
        keyFrom(new FieldElement('span'), new FieldElement('[contenteditable]:not([contenteditable="false"])')),
      ),
    ).toBe(true);
    expect(galleryKeyTargetsTextField(keyFrom(new FieldElement('span'), new FieldElement('wa-input')))).toBe(true);
    expect(galleryKeyTargetsTextField(keyFrom(new FieldElement('button')))).toBe(false);
    expect(galleryKeyTargetsTextField(keyFrom(new FieldElement('div')))).toBe(false);
  });
});
