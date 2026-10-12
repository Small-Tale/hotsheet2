import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { ChoiceCard } from './choice-card';

describe('ChoiceCard', () => {
  it('frames a native radio with its title, description, and brand tone by default', () => {
    const markup = String(
      ChoiceCard({
        type: 'radio',
        name: 'scope',
        value: 'all',
        checked: true,
        title: 'All',
        description: 'Everything.',
      }),
    );
    expect(markup).toContain('<label class="choice-card" data-component="choice-card" data-tone="brand">');
    expect(markup).toMatch(/<input type="radio" name="scope" value="all" checked[^>]*>/);
    expect(markup).not.toContain('disabled');
    expect(markup).toContain('<strong>All</strong>');
    expect(markup).toContain('<small>Everything.</small>');
  });

  it('renders checkbox, danger, disabled, and description-free presentations', () => {
    const markup = String(
      ChoiceCard({ type: 'checkbox', name: 'media', disabled: true, tone: 'danger', title: 'Media' }),
    );
    expect(markup).toContain('data-tone="danger"');
    expect(markup).toMatch(/<input type="checkbox" name="media"[^>]*disabled/);
    expect(markup).not.toContain(' checked');
    expect(markup).not.toContain('<small>');
  });

  it('owns its checked, danger, disabled, and full-width card styles', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'choice-card.css'), 'utf8');
    expect(css).toContainSource('.choice-card:has(> input[type]) { display: grid; width: auto; }');
    expect(css).toContain('.choice-card:has(input:checked)');
    expect(css).toContain(".choice-card[data-tone='danger']:has(input:checked)");
    expect(css).toContainSource('.choice-card:has(input:disabled) { cursor: not-allowed; opacity: 0.68; }');
    expect(css).not.toContain('.kui-');
  });
});
