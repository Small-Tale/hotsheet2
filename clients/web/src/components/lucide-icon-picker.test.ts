import { describe, expect, it, vi } from 'vitest';

import { loadLucideCatalog } from '../lucide-catalog';
import { LucideIconPicker } from './lucide-icon-picker';

describe('LucideIconPicker', () => {
  it('renders a search field and the curated popular icons by default, marking the resolved selection', () => {
    const markup = String(
      LucideIconPicker({ value: 'test', searchName: 'command-icon-search', selectAction: 'select-command-icon' }),
    );
    expect(markup).toContain('data-component="lucide-icon-picker"');
    expect(markup).toContain('name="command-icon-search"');
    // Popular defaults render as selectable buttons.
    expect(markup).toContain('data-action="select-command-icon"');
    expect(markup).toContain('data-icon-name="send"');
    // Legacy `test` resolves to test-tube-2 and shows pressed.
    expect(markup).toMatch(/data-icon-name="test-tube-2" aria-pressed="true"/);
  });

  it('shows a loading hint when searching before the catalog has loaded', async () => {
    // The catalog is cached in module state once any test loads it; a fresh module graph keeps
    // this test's "not loaded yet" precondition true in any test order (HS2-HBY8HN).
    vi.resetModules();
    const { isLucideCatalogLoaded } = await import('../lucide-catalog'),
      { LucideIconPicker: FreshPicker } = await import('./lucide-icon-picker');
    expect(isLucideCatalogLoaded()).toBe(false);
    const markup = String(
      FreshPicker({ query: 'compass', searchName: 'command-icon-search', selectAction: 'select-command-icon' }),
    );
    expect(markup).toContain('Loading icons…');
  });

  it('searches the full catalog once loaded and reports an empty result', async () => {
    await loadLucideCatalog();
    const markup = String(
      LucideIconPicker({ query: 'compass', searchName: 'command-icon-search', selectAction: 'select-command-icon' }),
    );
    expect(markup).toContain('data-icon-name="compass"');
    expect(markup).not.toContain('Loading icons…');
    const empty = String(
      LucideIconPicker({
        query: 'definitely-not-an-icon-zzz',
        searchName: 'command-icon-search',
        selectAction: 'select-command-icon',
      }),
    );
    expect(empty).toContain('No icons match');
  });
});
