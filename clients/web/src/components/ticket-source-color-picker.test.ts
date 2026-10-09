import { describe, expect, it } from 'vitest';

import { ProviderSetupForm } from './provider-setup-form';
import { TicketSourceColorPicker } from './ticket-source-color-picker';

describe('TicketSourceColorPicker', () => {
  it('previews every color on the actual source mark and selects the saved color', () => {
    const markup = String(
      TicketSourceColorPicker({ source: { provider: 'github', name: 'Issues', color: '#3b82f6' } }),
    );
    expect(markup.match(/type="radio"/g)).toHaveLength(8);
    expect(markup.match(/data-provider="github"/g)).toHaveLength(8);
    expect(markup).toContain('--ticket-source-choice-color: #3b82f6');
    expect(markup).toMatch(/value="#3b82f6"[^>]*checked/);
    expect(markup).not.toContain('>Transparent</span>');
    expect(markup).toContain('>Blue</span>');
  });

  it('selects Gray when a source has no color or a legacy transparent value (HS2-H1FZNV)', () => {
    for (const color of [undefined, 'transparent']) {
      const markup = String(TicketSourceColorPicker({ source: { provider: 'git', name: 'Local', color } }));
      expect(markup).toMatch(/value="#6b7280"[^>]*checked/);
      expect(markup).not.toContain('value="transparent"');
    }
  });

  it('keeps project-local color inside the external connection editor', () => {
    const markup = String(
      ProviderSetupForm({
        kind: 'github',
        connection: {
          id: 'github-main',
          provider: 'github',
          locator: 'acme/issues',
          name: 'Issues',
          default: false,
          settings: {},
        },
        sourceColor: '#8b5cf6',
      }),
    );
    expect(markup).toContain('data-component="ticket-source-color-picker"');
    expect(markup).toMatch(/value="#8b5cf6"[^>]*checked/);
    expect(markup).not.toContain('data-source-id=');
    expect(markup).toContain('This icon color applies only to this project.');
  });
});
