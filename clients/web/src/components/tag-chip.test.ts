import { describe, expect, it } from 'vitest';
import { normalizeTagChipProps } from './tag-chip';

describe('TagChip', () => {
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
