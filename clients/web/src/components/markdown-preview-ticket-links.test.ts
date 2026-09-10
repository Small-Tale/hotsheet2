import { describe, expect, it } from 'vitest';

import { linkTicketReferences } from './markdown-preview';

describe('Markdown ticket links', () => {
  it('preserves the production hook and adds an explicit project ID for cross-project references', () => {
    const markup = linkTicketReferences('Local HS2-LOCAL1 and cross-project @beta-02/HS2-REMOTE1.');
    expect(markup.match(/data-action="open-linked-ticket"/g)).toHaveLength(2);
    expect(markup).toContain('data-ticket-slug="HS2-LOCAL1"');
    expect(markup).toContain('data-ticket-slug="HS2-REMOTE1" data-ticket-project-id="beta-02"');
    expect(markup).toContain('>@beta-02/HS2-REMOTE1</a>');
  });

  it('does not link references inside existing links, buttons, or code', () => {
    const markup = linkTicketReferences('<a href="/ticket">HS2-LINKED1</a><button>HS2-BUTTON1</button><code>@beta-02/HS2-CODE01</code> HS2-PLAIN1');
    expect(markup.match(/data-action="open-linked-ticket"/g)).toHaveLength(1);
    expect(markup).toContain('data-ticket-slug="HS2-PLAIN1"');
  });
});
