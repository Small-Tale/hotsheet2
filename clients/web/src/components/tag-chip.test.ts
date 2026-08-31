import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { normalizeTagChipProps } from './tag-chip';

describe('TagChip', () => {
  it('keeps the removable control compact inside inspector-sized chips', () => {
    const tagChipCss = readFileSync(resolve(import.meta.dirname, 'tag-chip.css'), 'utf8');
    expect(tagChipCss).toContain('::part(remove-button__base)');
    expect(tagChipCss).toContain('width: 1rem');
    expect(tagChipCss).toContain('font-size: .65rem');
  });
  it('provides stable compact defaults', () => {
    expect(normalizeTagChipProps({ id: 'tag-1', label: ' needs-design ' })).toEqual({
      id: 'tag-1', label: 'needs-design', variant: 'neutral', appearance: 'filled',
      size: 'small', removable: false, pill: false, disabled: false,
    });
  });

  it('preserves explicit presentation and supplies a readable empty-label fallback', () => {
    expect(normalizeTagChipProps({
      id: 'tag-2', label: ' ', variant: 'danger', appearance: 'accent', size: 'large',
      removable: true, pill: false, disabled: true,
    })).toMatchObject({ label: 'Untitled tag', variant: 'danger', appearance: 'accent', size: 'large', removable: true, pill: false, disabled: true });
  });
});
