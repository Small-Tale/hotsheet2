import { describe, expect, it } from 'vitest';
import { createDevApp } from '../dev-server';
import { demoCatalog, findDemo, flattenCatalog } from './catalog';
import { resetTagChipDemo, tagChipSettings } from './tag-chip-demo';

describe('UX demo catalog', () => {
  it('has unique routes and one implemented initial component', () => {
    const entries = flattenCatalog(demoCatalog);
    expect(new Set(entries.map(entry => entry.id)).size).toBe(entries.length);
    expect(entries.filter(entry => entry.implemented).map(entry => entry.id)).toEqual(['tag-chip']);
    expect(findDemo('tag-chip')?.name).toBe('TagChip');
  });

  it('serves UX markup only when development is explicitly enabled', async () => {
    const dev = await createDevApp(true).request('/ux-demo');
    expect(dev.status).toBe(200);
    expect(await dev.text()).toContain('/src/ux-demo/main.tsx');
    expect((await createDevApp(false).request('/ux-demo')).status).toBe(404);
  });

  it('resets every canonical TagChip demo setting', () => {
    tagChipSettings.label.value = 'changed';
    tagChipSettings.variant.value = 'danger';
    tagChipSettings.appearance.value = 'accent';
    tagChipSettings.size.value = 'large';
    tagChipSettings.removable.value = false;
    tagChipSettings.pill.value = false;
    tagChipSettings.disabled.value = true;
    tagChipSettings.event.value = 'Changed';
    resetTagChipDemo();
    expect({
      label: tagChipSettings.label.value, variant: tagChipSettings.variant.value,
      appearance: tagChipSettings.appearance.value, size: tagChipSettings.size.value,
      removable: tagChipSettings.removable.value, pill: tagChipSettings.pill.value,
      disabled: tagChipSettings.disabled.value, event: tagChipSettings.event.value,
    }).toEqual({
      label: 'needs-design', variant: 'neutral', appearance: 'filled-outlined', size: 'small',
      removable: true, pill: true, disabled: false, event: 'No actions yet',
    });
  });
});
