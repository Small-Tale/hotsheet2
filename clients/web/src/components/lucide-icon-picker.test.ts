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
    // Popular defaults render as Kerf's native icon radios.
    expect(markup).toContain('data-component="choice-chips"');
    expect(markup).toContain('data-presentation="icons"');
    expect(markup).toContain('data-action="select-command-icon"');
    expect(markup).toContain('data-choice-value="send"');
    // Legacy `test` resolves to test-tube-2 and checks its native radio.
    expect(markup).toMatch(/type="radio" name="command-icon-choice" value="test-tube-2"[^>]*checked/);
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
    expect(markup).toContain('data-choice-value="compass"');
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
