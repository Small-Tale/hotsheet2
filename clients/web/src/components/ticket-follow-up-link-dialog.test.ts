import { describe, expect, it } from 'vitest';

import { TicketFollowUpLinkDialog } from './ticket-follow-up-link-dialog';

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
  id: 'target',
  slug: 'HS2-TARGET',
  title: 'Synthetic target',
  projectId: 'beta',
  projectName: 'Beta',
  connectionId: 'git-beta',
  nativeId: 'target',
  qualifiedId: 'git-beta:target',
};
const disabledSubmit = /<button(?=[^>]*type="submit")(?=[^>]*disabled)[^>]*>/;

describe('TicketFollowUpLinkDialog (HS2-MCGCPQ)', () => {
  it('renders nothing while closed', () => {
    expect(String(TicketFollowUpLinkDialog({}))).toBe('');
  });

  it('names the source, searches with the shared picker, and hides the source from results', () => {
    const markup = String(
      TicketFollowUpLinkDialog({
        state: { source, existing: [], query: 'synthetic', candidates: [source, target] },
      }),
    );
    expect(markup).toContain('Link Follow-Up to HS2-SOURCE');
    expect(markup).toContain('data-dialog-component="ticket-follow-up-link-dialog"');
    expect(markup).toContain('data-component="ticket-reference-picker"');
    expect(markup).toContain('name="follow-up-link-target-search"');
    expect(markup).toContain('data-action="select-follow-up-link-target" data-item-id="beta::git-beta%3Atarget"');
    expect(markup).not.toContain('data-item-id="alpha::git-alpha%3Asource"');
    expect(markup).toContain('data-action="submit-follow-up-link"');
    expect(markup).toContain('Select the existing ticket to link as a follow-up.');
    expect(markup).toMatch(disabledSubmit);
    // Cancel leads; the decision trails.
    expect(markup.indexOf('data-action="cancel-follow-up-link"')).toBeLessThan(markup.indexOf('Link Follow-Up</'));
  });

  it('enables Link Follow-Up once a target is selected and offers Change', () => {
    const markup = String(
      TicketFollowUpLinkDialog({ state: { source, existing: [], query: '', candidates: [], selected: target } }),
    );
    expect(markup).toContain('data-action="clear-follow-up-link-target"');
    expect(markup).toContain('Synthetic target');
    expect(markup).not.toMatch(disabledSubmit);
    expect(markup).not.toContain('already a follow-up');
  });

  it('says an already-linked target changes nothing but still allows the idempotent link', () => {
    const markup = String(
      TicketFollowUpLinkDialog({
        state: { source, existing: ['beta::git-beta%3Atarget'], query: '', candidates: [], selected: target },
      }),
    );
    expect(markup).toContain('HS2-TARGET is already a follow-up of HS2-SOURCE. Linking it again changes nothing.');
    expect(markup).not.toMatch(disabledSubmit);
  });

  it('shows a refusal and the busy state', () => {
    const refused = String(
      TicketFollowUpLinkDialog({
        state: { source, existing: [], query: '', candidates: [], selected: target, error: 'Store unavailable.' },
      }),
    );
    expect(refused).toMatch(/role="alert"[^]*Store unavailable\./);
    const busy = String(
      TicketFollowUpLinkDialog({
        state: { source, existing: [], query: '', candidates: [], selected: target, submitting: true },
      }),
    );
    expect(busy).toContain('Linking…');
    expect(busy).toMatch(disabledSubmit);
  });
});
