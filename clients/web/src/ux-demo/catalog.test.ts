import { describe, expect, it } from 'vitest';
import { createDevApp } from '../dev-server';
import { demoCatalog, findDemo, flattenCatalog } from './catalog';

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
});
