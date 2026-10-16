import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { DemoSettingsForm, DemoSettingsPreview } from './demo-settings-form';

describe('demo settings compositions (HS2-3AZ08H)', () => {
  it('stacks an entry settings form in a Kerf List with control insets', () => {
    const markup = String(DemoSettingsForm({ settings: 'status-badge', children: undefined }));
    expect(markup).toMatch(
      /^<form data-settings="status-badge"><div class="kui-list" data-component="list"[^>]*data-control-insets="rbl"/u,
    );
    expect(markup).not.toContain('settings-form');
  });

  it('renders the Project Settings stand-in as a framed Kerf surface of Text', () => {
    const markup = String(DemoSettingsPreview({ description: 'Configure the project.' }));
    expect(markup).toMatch(/^<section aria-label="Project Settings"><div class="kui-list"[^>]*data-inset="document"/u);
    expect(markup).toMatch(/data-component="content-item"[^>]*data-appearance="surface"/u);
    expect(markup).toMatch(/<h2 class="kui-text"[^>]*>Project Settings<\/h2>/u);
    expect(markup).toContain('Configure the project.');
  });

  it('keeps no generic form, card, or placeholder rules in the catalog stylesheet', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'style.css'), 'utf8');
    for (const selector of ['.settings-form', '.workspace-settings-preview', '.shell-mode-surface'])
      expect(css, selector).not.toContain(selector);
  });
});
