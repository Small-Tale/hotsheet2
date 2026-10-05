import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { normalizeTagChipProps, TAG_CHIP_REMOVE_ACTION, TagChip } from './tag-chip';

describe('TagChip', () => {
  it("renders Kerf's Chip inside an identity wrapper and owns no chip chrome", () => {
    const tagChipCss = readFileSync(resolve(import.meta.dirname, 'tag-chip.css'), 'utf8');
    expect(tagChipCss).not.toContain('wa-tag');
    expect(tagChipCss).not.toContain('::part(');
    expect(tagChipCss).not.toContain('.kui-chip');
    const html = String(TagChip({ id: 'tag-1', label: 'needs-design', removable: true }));
    expect(html).toContain('data-component="tag-chip" data-tag-id="tag-1" data-disabled="false"');
    expect(html).toContain('data-component="chip"');
    expect(html).toContain('data-tone="neutral"');
    expect(html).toContain('data-appearance="quiet"');
    expect(html).toContain('data-shape="rounded"');
    expect(html).toContain('data-size="compact"');
    expect(html).toContain('data-item-id="tag-1"');
    expect(html).toContain(`data-action="${TAG_CHIP_REMOVE_ACTION}"`);
    expect(html).toContain('aria-label="Remove needs-design"');
  });

  it('maps Hot Sheet presentation onto Chip tone, appearance, shape, and size', () => {
    expect(String(TagChip({ id: 'brand', label: 'brand', variant: 'brand' }))).toContain('data-tone="info"');
    const accent = String(
      TagChip({ id: 'a', label: 'a', variant: 'danger', appearance: 'accent', size: 'large', pill: true }),
    );
    expect(accent).toContain('data-tone="danger"');
    expect(accent).toContain('data-appearance="solid"');
    expect(accent).toContain('data-shape="pill"');
    expect(accent).toContain('data-size="default"');
    expect(accent).not.toContain('data-action=');
    expect(String(TagChip({ id: 'b', label: 'b', appearance: 'outlined' }))).toContain('data-appearance="outline"');
    expect(String(TagChip({ id: 'c', label: 'c', appearance: 'filled-outlined' }))).toContain(
      'data-appearance="outline"',
    );
    const disabled = String(TagChip({ id: 'd', label: 'd', removable: true, disabled: true }));
    expect(disabled).toContain('data-disabled="true"');
    expect(disabled).toMatch(/<button[^>]*data-action="remove-tag-chip"[^>]*disabled/);
  });
  it('provides stable compact defaults', () => {
    expect(normalizeTagChipProps({ id: 'tag-1', label: ' needs-design ' })).toEqual({
      id: 'tag-1',
      label: 'needs-design',
      variant: 'neutral',
      appearance: 'filled',
      size: 'small',
      removable: false,
      pill: false,
      disabled: false,
    });
  });

  it('preserves explicit presentation and supplies a readable empty-label fallback', () => {
    expect(
      normalizeTagChipProps({
        id: 'tag-2',
        label: ' ',
        variant: 'danger',
        appearance: 'accent',
        size: 'large',
        removable: true,
        pill: false,
        disabled: true,
      }),
    ).toMatchObject({
      label: 'Untitled tag',
      variant: 'danger',
      appearance: 'accent',
      size: 'large',
      removable: true,
      pill: false,
      disabled: true,
    });
  });
});
