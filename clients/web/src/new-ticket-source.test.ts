import { describe, expect, it } from 'vitest';

import { type Capabilities, type ProviderDescriptor } from './api';
import {
  projectTicketSources,
  rememberNewTicketSource,
  resolveNewTicketSource,
  writableTicketSources,
} from './new-ticket-source';

const capabilities = (create: boolean) => ({ create, attachments: create }) as Capabilities;
const descriptor = (connection_id: string, create: boolean, isDefault = false): ProviderDescriptor => ({
  connection_id,
  provider: connection_id.startsWith('git') ? 'git' : 'github',
  display_name: `${connection_id} tickets`,
  locator: `/${connection_id}`,
  default: isDefault,
  capabilities: capabilities(create),
});

describe('new ticket source selection (HS2-NZMJBJ)', () => {
  it('projects the default source and every addressable source', () => {
    expect(projectTicketSources([])).toBeUndefined();
    const sources = projectTicketSources([descriptor('git-a', true), descriptor('github-b', true, true)])!;
    expect(sources.name).toBe('github-b tickets');
    expect(sources.connectionId).toBe('github-b');
    expect(sources.sources.map((item) => [item.connectionId, item.default])).toEqual([
      ['git-a', false],
      ['github-b', true],
    ]);
    // Without an explicit default the first descriptor is the default, as before.
    expect(projectTicketSources([descriptor('git-a', true), descriptor('github-b', true)])!.connectionId).toBe('git-a');
  });

  it('offers only sources that can create tickets', () => {
    const sources = projectTicketSources([
      descriptor('git-a', true, true),
      descriptor('jira-ro', false),
      descriptor('github-b', true),
    ]);
    expect(writableTicketSources(sources).map((item) => item.connectionId)).toEqual(['git-a', 'github-b']);
    expect(writableTicketSources(undefined)).toEqual([]);
  });

  it('prefers the last-used writable source, then the default, then the first writable one', () => {
    const sources = projectTicketSources([
      descriptor('git-a', true, true),
      descriptor('jira-ro', false),
      descriptor('github-b', true),
    ]);
    expect(resolveNewTicketSource(sources)?.connectionId).toBe('git-a');
    expect(resolveNewTicketSource(sources, 'github-b')?.connectionId).toBe('github-b');
    // A remembered source that was removed or became read-only falls back to the default.
    expect(resolveNewTicketSource(sources, 'gone')?.connectionId).toBe('git-a');
    expect(resolveNewTicketSource(sources, 'jira-ro')?.connectionId).toBe('git-a');
    const readOnlyDefault = projectTicketSources([descriptor('jira-ro', false, true), descriptor('github-b', true)]);
    expect(resolveNewTicketSource(readOnlyDefault)?.connectionId).toBe('github-b');
    // With nothing writable the default still answers, so callers see create: false.
    const readOnly = projectTicketSources([descriptor('jira-ro', false, true)]);
    expect(resolveNewTicketSource(readOnly)).toMatchObject({
      connectionId: 'jira-ro',
      capabilities: { create: false },
    });
    expect(resolveNewTicketSource(undefined)).toBeUndefined();
  });

  it('remembers the last-used source per project without touching other projects', () => {
    let remembered: Record<string, string> = {};
    remembered = rememberNewTicketSource(remembered, 'p1', 'github-b');
    remembered = rememberNewTicketSource(remembered, 'p2', 'git-a');
    const before = remembered;
    remembered = rememberNewTicketSource(remembered, 'p1', 'git-a');
    expect(remembered).toEqual({ p1: 'git-a', p2: 'git-a' });
    expect(before).toEqual({ p1: 'github-b', p2: 'git-a' });
  });
});
