import { describe, expect, it } from 'vitest';

import { GROUP_REGISTRY_SYNC_STATES, type GroupRegistrySync } from '../server-response-parsers';
import {
  groupRegistryNeedsAttention,
  GroupRegistrySyncBanner,
  groupRegistrySyncSentence,
} from './group-registry-sync-banner';

const status = (state: GroupRegistrySync['state'], detail = 'detail'): GroupRegistrySync => ({
  state,
  detail,
  committed: false,
  at: '2026-11-13T00:40:00Z',
});

describe('groupRegistryNeedsAttention (HS2-VFYSQK)', () => {
  it('flags only blocked and failed, mirroring the server’s needs_attention', () => {
    const flagged = GROUP_REGISTRY_SYNC_STATES.filter((state) => groupRegistryNeedsAttention(status(state)));
    expect(flagged).toEqual(['blocked', 'failed']);
    expect(groupRegistryNeedsAttention(undefined)).toBe(false);
  });
});

describe('groupRegistrySyncSentence (HS2-VFYSQK)', () => {
  it('capitalizes the server’s line and ends it once', () => {
    expect(groupRegistrySyncSentence('the upstream moved; pull (with --rebase) to publish the group registry')).toBe(
      'The upstream moved; pull (with --rebase) to publish the group registry.',
    );
    expect(groupRegistrySyncSentence('  already a sentence.  ')).toBe('Already a sentence.');
    expect(groupRegistrySyncSentence('truncated…')).toBe('Truncated…');
    expect(groupRegistrySyncSentence('   ')).toBe('');
  });
});

describe('GroupRegistrySyncBanner (HS2-VFYSQK)', () => {
  it('renders nothing while the registry publishes, waits, stays local, or is not published', () => {
    expect(GroupRegistrySyncBanner({ status: undefined })).toBeUndefined();
    for (const state of ['synced', 'local_only', 'waiting', 'untracked'] as const)
      expect(GroupRegistrySyncBanner({ status: status(state) })).toBeUndefined();
  });

  it('shows a failed publication with the server’s detail and the automatic retry', () => {
    const html = String(
      GroupRegistrySyncBanner({
        status: status('failed', 'could not commit the group registry: pre-commit hook refused the commit'),
      }),
    );
    expect(html).toContain('data-component="group-registry-sync-banner"');
    expect(html).toContain('data-tone="warning"');
    expect(html).toContain('Ticket groups are not publishing changes');
    expect(html).toContain('Could not commit the group registry: pre-commit hook refused the commit.');
    expect(html).toContain('other clones do not see new or renamed groups');
    expect(html).toContain('Hot Sheet tries again on its next sync.');
    expect(html).toContain('cloud-alert');
  });

  it('shows a blocked publication with the server’s next step and no retry promise', () => {
    const html = String(
      GroupRegistrySyncBanner({
        status: status(
          'blocked',
          'the branch is behind its upstream; pull (with --rebase) to publish the group registry',
        ),
      }),
    );
    expect(html).toContain('The branch is behind its upstream; pull (with --rebase) to publish the group registry.');
    expect(html).not.toContain('tries again');
  });
});
