import { describe, expect, it } from 'vitest';

import { TicketBlockerDialog } from './ticket-blocker-dialog';

const source = {
  id: 'source',
  slug: 'HS2-SOURCE',
  title: 'Synthetic source',
  projectId: 'alpha',
  projectName: 'Alpha',
  connectionId: 'git-alpha',
  nativeId: 'source',
  qualifiedId: 'git-alpha:source',
};
const target = {
  ...source,
  id: 'one',
  slug: 'HS2-ONE',
  title: 'Synthetic one',
  nativeId: 'one',
  qualifiedId: 'git-alpha:one',
};
const foreign = { ...target, connectionId: 'gh-alpha', qualifiedId: 'gh-alpha:one', slug: 'GH-1' };
const disabledSubmit = /<button(?=[^>]*type="submit")(?=[^>]*disabled)[^>]*>/;

describe('TicketBlockerDialog (HS2-CHH28E)', () => {
  it('renders nothing while closed', () => {
    expect(String(TicketBlockerDialog({}))).toBe('');
  });

  it('names the source and offers only same-source tickets other than itself', () => {
    const markup = String(
      TicketBlockerDialog({
        state: { source, existing: [], query: 'synthetic', candidates: [source, target, foreign] },
      }),
    );
    expect(markup).toContain('Add Blocker to HS2-SOURCE');
    expect(markup).toContain('data-dialog-component="ticket-blocker-dialog"');
    expect(markup).toContain('name="blocker-target-search"');
    expect(markup).toContain('data-action="select-blocker" data-item-id="alpha::git-alpha%3Aone"');
    expect(markup).not.toContain('alpha::git-alpha%3Asource');
    expect(markup).not.toContain('GH-1');
    expect(markup).toMatch(disabledSubmit);
    // Nothing chosen yet is not an error to announce.
    expect(markup).not.toContain('Select the ticket that blocks');
    expect(markup.indexOf('data-action="cancel-blocker"')).toBeLessThan(markup.indexOf('Add Blocker</'));
  });

  it('enables Add Blocker for a valid choice and refuses an existing blocker', () => {
    const chosen = String(
      TicketBlockerDialog({ state: { source, existing: [], query: '', candidates: [], selected: target } }),
    );
    expect(chosen).toContain('data-action="clear-blocker"');
    expect(chosen).not.toMatch(disabledSubmit);
    const existing = String(
      TicketBlockerDialog({ state: { source, existing: ['one'], query: '', candidates: [], selected: target } }),
    );
    expect(existing).toContain('HS2-ONE already blocks HS2-SOURCE.');
    expect(existing).toMatch(disabledSubmit);
  });

  it('shows a server refusal in the alert and a busy submit while adding', () => {
    const markup = String(
      TicketBlockerDialog({
        state: {
          source,
          existing: [],
          query: '',
          candidates: [],
          selected: target,
          submitting: true,
          error: 'HS2-ONE already depends on HS2-SOURCE (dependency cycle)',
        },
      }),
    );
    expect(markup).toMatch(/role="alert"[\s\S]*dependency cycle/);
    expect(markup).toContain('Adding…');
    expect(markup).toMatch(disabledSubmit);
  });
});
