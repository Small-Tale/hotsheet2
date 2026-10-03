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

  it('links a bare legacy HS-N reference like any other slug (HS2-XB5R3Y)', () => {
    const markup = linkTicketReferences('See HS-1234 for history.');
    expect(markup.match(/data-action="open-linked-ticket"/g)).toHaveLength(1);
    expect(markup).toContain('data-ticket-slug="HS-1234"');
    expect(markup).toContain('>HS-1234</a>');
  });

  it('links single-digit legacy references without linking arbitrary short tokens (HS2-T9TVYT)', () => {
    const markup = linkTicketReferences('See HS-7, but leave AB-1 and HS-A alone.');
    expect(markup.match(/data-action="open-linked-ticket"/g)).toHaveLength(1);
    expect(markup).toContain('data-ticket-slug="HS-7"');
    expect(markup).toContain('AB-1 and HS-A');
  });

  it('links an inline code span that is exactly one ticket reference, keeping the code chip (HS2-5T33YV)', () => {
    const markup = linkTicketReferences(
      '<p>FEEDBACK NEEDED: intended from <code>HS2-3SCH1K</code>; see <code>@beta-02/HS2-REMOTE1</code>.</p>',
    );
    expect(markup.match(/data-action="open-linked-ticket"/g)).toHaveLength(2);
    expect(markup).toContain(
      '<a class="markdown-preview__ticket-reference" href="#ticket-HS2-3SCH1K" data-action="open-linked-ticket" data-ticket-slug="HS2-3SCH1K" title="Open HS2-3SCH1K"><code>HS2-3SCH1K</code></a>',
    );
    expect(markup).toContain('data-ticket-slug="HS2-REMOTE1" data-ticket-project-id="beta-02"');
    expect(markup).toContain('><code>@beta-02/HS2-REMOTE1</code></a>');
    // The visible text is unchanged, so inline feedback reply offsets stay aligned.
    expect(markup.replace(/<[^>]+>/g, '')).toBe('FEEDBACK NEEDED: intended from HS2-3SCH1K; see @beta-02/HS2-REMOTE1.');
  });

  it('keeps code with other text, code blocks, and linked code inert, and resumes linking after them', () => {
    const markup = linkTicketReferences(
      [
        '<p><code>hotsheet-cli show HS2-MIXED1</code> <code> HS2-SPACE1 </code></p>',
        '<pre><code>HS2-BLOCK1</code></pre>',
        '<p><a href="/x"><code>HS2-INLINK</code></a> <code></code> then HS2-AFTER1 and <code>HS2-AFTER2</code></p>',
      ].join(''),
    );
    expect(markup.match(/data-action="open-linked-ticket"/g)).toHaveLength(2);
    expect(markup).toContain('data-ticket-slug="HS2-AFTER1"');
    expect(markup).toContain('data-ticket-slug="HS2-AFTER2"');
    for (const inert of ['HS2-MIXED1', 'HS2-SPACE1', 'HS2-BLOCK1', 'HS2-INLINK'])
      expect(markup).not.toContain(`data-ticket-slug="${inert}"`);
  });

  it('does not link references inside existing links, buttons, or code that holds other text', () => {
    const markup = linkTicketReferences(
      '<a href="/ticket">HS2-LINKED1</a><button>HS2-BUTTON1</button><code>open @beta-02/HS2-CODE01</code> HS2-PLAIN1',
    );
    expect(markup.match(/data-action="open-linked-ticket"/g)).toHaveLength(1);
    expect(markup).toContain('data-ticket-slug="HS2-PLAIN1"');
  });
});
