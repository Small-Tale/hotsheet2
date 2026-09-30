import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { providerName, ProviderSetupForm } from './provider-setup-form';
import { TicketSourceSetupDialog } from './ticket-source-setup-dialog';
import { TicketSourcesSettings } from './ticket-sources-settings';

describe('ticket source surfaces', () => {
  it('renders the setup root and preserves all five delegated actions', () => {
    const markup = String(
      TicketSourceSetupDialog({
        project: { root: '/work/demo', name: 'Demo', stores: [], needsTicketSetup: true },
        providerConnections: [],
        navigation: 'none',
      }),
    );
    expect(markup).toContain('data-component="ticket-source-setup-dialog"');
    expect(markup).toContain('Demo</strong> is open');
    for (const action of ['create-project-git-source', 'create-project-git-source-custom', 'select-provider-kind'])
      expect(markup).toContain(`data-action="${action}"`);
    for (const label of [
      'Create a Hot Sheet 2 git ticket repository',
      'Connect GitHub Issues',
      'Connect GitLab Issues',
      'Connect Jira Cloud',
    ])
      expect(markup).toContain(label);
  });

  it('starts from sign-in with an explicit GitHub Enterprise path (HS2-1JT25R)', () => {
    const form = (auth?: import('./provider-setup-form').GithubAuthState) =>
      String(ProviderSetupForm({ kind: 'github', auth }));
    const idle = form();
    expect(idle).toContain('data-action="start-github-sign-in"');
    expect(idle).toContain('Sign in with GitHub</wa-button>');
    expect(idle).toContain('data-component="sunken-panel"');
    expect(idle).toContain('appearance="accent"');
    expect(idle).toContain('data-action="choose-github-enterprise"');
    expect(idle).toContain('copies your one-time code');
    expect(idle).not.toContain('data-component="grid"');
    expect(idle).not.toContain('name="api-base"');
    const blank = { session: '', userCode: '', verificationUri: '' };
    const enterprise = form({ ...blank, state: 'idle', enterprise: true, enterpriseUrl: 'https://ghe.test' });
    expect(enterprise).toContain('GitHub Enterprise server URL');
    expect(enterprise).toContain('name="github-enterprise-url"');
    expect(enterprise).toContain('label="GitHub Enterprise server URL"');
    expect(enterprise).toContain('value="https://ghe.test"');
    expect(enterprise).toContain('Sign in with GitHub Enterprise');
    expect(enterprise).toContain('data-action="choose-github-dotcom"');
    const copied = form({ session: 's', userCode: 'WXYZ-1234', verificationUri: 'u', state: 'waiting', copied: true });
    expect(copied).toContain('it is already on your clipboard');
    expect(copied).toContain('Copy again');
    expect(
      form({ ...blank, session: 's', state: 'authorized', enterpriseUrl: 'https://ghe.test', repositories: [] }),
    ).toContain('Signed in to ghe.test.');
    const denied = form({ ...blank, session: 's', state: 'denied' });
    expect(denied).toContain('GitHub sign in was denied. Try again.');
    expect(denied).toContain('data-action="start-github-sign-in"');
    // Connect stays disabled until GitHub authorizes; editing never needs sign-in.
    const dialog = (githubAuth?: import('./provider-setup-form').GithubAuthState, editing = false) =>
      String(
        TicketSourceSetupDialog({
          project: { root: '/w', name: 'W', stores: [] },
          providerKind: 'github',
          providerConnections: [
            { id: 'gh', provider: 'github', locator: 'a/b', name: null, default: false, settings: {} },
          ],
          editingProviderId: editing ? 'gh' : undefined,
          githubAuth,
          navigation: 'none',
        }),
      );
    expect(dialog()).toMatch(/data-action="submit-provider-setup" disabled/);
    expect(dialog({ ...blank, session: 's', state: 'authorized' })).not.toMatch(
      /data-action="submit-provider-setup" disabled/,
    );
    expect(dialog(undefined, true)).not.toMatch(/data-action="submit-provider-setup" disabled/);
  });

  it('lists every reachable repository for search and explains missing ones (HS2-27T5WT)', () => {
    const form = (auth: Partial<import('./provider-setup-form').GithubAuthState>) =>
      String(
        ProviderSetupForm({
          kind: 'github',
          auth: { session: 's', userCode: 'C', verificationUri: 'https://github.test', state: 'authorized', ...auth },
        }),
      );
    const limited = form({
      repositories: ['acme/one', 'acme/two'],
      installations: [
        { account: 'acme', selection: 'all' },
        { account: 'me', selection: 'selected', settingsUrl: 'https://github.test/settings/installations/7' },
      ],
      installUrl: 'https://github.test/apps/hot-sheet/installations/new',
    });
    expect(limited).toContain('list="provider-setup-github-repositories"');
    expect(limited).toContain('<option value="acme/one"></option>');
    expect(limited).toContain('2 repositories available.');
    expect(limited).toContain('you chose on <strong>me</strong>');
    expect(limited).not.toContain('<strong>acme</strong>');
    expect(limited).toContain('href="https://github.test/settings/installations/7"');
    expect(limited).toContain('Add another account or organization');
    expect(limited).toContain('data-action="refresh-github-repositories"');
    expect(limited).not.toContain('<select');
    const none = form({ repositories: [], installations: [] });
    expect(none).toContain('not installed on any account yet');
    expect(none).toContain('0 repositories available.');
    expect(form({ repositories: ['a/b'], installations: [], refreshing: true })).toMatch(
      /refresh-github-repositories" disabled[\s\S]*?Refreshing…/,
    );
    // Before the list arrives there is nothing to search yet.
    expect(form({})).not.toContain('provider-setup-github-repositories');
  });

  it('toggles availability while editing and marks disabled connections in settings (HS2-SF6W34)', () => {
    const connection = {
        id: 'github-main',
        provider: 'github',
        locator: 'acme/repo',
        name: 'Product issues',
        default: false,
        settings: {},
      },
      dialog = (disabled: boolean) =>
        String(
          TicketSourceSetupDialog({
            project: { root: '/work/demo', name: 'Demo', stores: [] },
            providerKind: 'github',
            providerConnections: [{ ...connection, disabled }],
            editingProviderId: 'github-main',
            navigation: 'none',
          }),
        );
    expect(dialog(false)).toContain('data-action="toggle-provider-disabled"');
    expect(dialog(false)).toContain('data-lucide="power-off"');
    expect(dialog(false)).toMatch(/power-off[\s\S]*?Disable<\/wa-button>/);
    expect(dialog(true)).toContain('data-lucide="power"');
    expect(dialog(true)).toMatch(/Enable<\/wa-button>/);
    const connecting = String(
      TicketSourceSetupDialog({
        project: { root: '/work/demo', name: 'Demo', stores: [] },
        providerKind: 'github',
        providerConnections: [],
        navigation: 'none',
      }),
    );
    expect(connecting).not.toContain('toggle-provider-disabled');
    const settings = (disabled: boolean) =>
      String(TicketSourcesSettings({ stores: [], providerConnections: [{ ...connection, disabled }] }));
    expect(settings(true)).toContain('<small data-state="disabled">Disabled</small>');
    expect(settings(false)).not.toContain('data-state="disabled"');
  });

  it('offers permanent removal only while editing, behind an inline confirmation (HS2-724S9N)', () => {
    const connection = {
        id: 'github-main',
        provider: 'github',
        locator: 'acme/repo',
        name: 'Product issues',
        default: true,
        settings: {},
      },
      base = {
        project: { root: '/work/demo', name: 'Demo', stores: [] },
        providerKind: 'github' as const,
        providerConnections: [connection],
        navigation: 'none' as const,
      };
    const connecting = String(TicketSourceSetupDialog(base));
    expect(connecting).not.toContain('request-provider-removal');
    const editing = String(TicketSourceSetupDialog({ ...base, editingProviderId: 'github-main' }));
    expect(editing).toContain('data-action="request-provider-removal"');
    expect(editing).toContain('Remove data source…');
    expect(editing).toContain('data-lucide="trash-2"');
    expect(editing).toContain('data-action="submit-provider-setup"');
    expect(editing).not.toContain('confirm-provider-removal');
    const confirming = String(
      TicketSourceSetupDialog({ ...base, editingProviderId: 'github-main', removingProviderId: 'github-main' }),
    );
    expect(confirming).toContain('<strong>Remove Product issues?</strong>');
    expect(confirming).toContain('Tickets stay in GitHub Issues.');
    expect(confirming).toContain('data-action="confirm-provider-removal"');
    expect(confirming).toContain('data-action="cancel-provider-removal"');
    expect(confirming).not.toContain('data-action="submit-provider-setup"');
    // A stale confirmation for another connection never arms this one.
    const stale = String(
      TicketSourceSetupDialog({ ...base, editingProviderId: 'github-main', removingProviderId: 'github-old' }),
    );
    expect(stale).not.toContain('confirm-provider-removal');
    const busy = String(
      TicketSourceSetupDialog({
        ...base,
        editingProviderId: 'github-main',
        removingProviderId: 'github-main',
        providerBusy: true,
      }),
    );
    expect(busy).toContain('Removing…');
  });

  it('renders provider editing, GitHub authorization, and remote-backup states from props', () => {
    const auth = String(
      ProviderSetupForm({
        kind: 'github',
        auth: {
          session: 'session',
          userCode: 'ABCD',
          verificationUri: 'https://github.com/login/device',
          state: 'waiting',
        },
        error: 'Try again',
      }),
    );
    expect(auth).toContain('Waiting for GitHub…');
    expect(auth).toContain('ABCD');
    expect(auth).toContain('Try again');
    // Sign in comes first: a new GitHub connection's settings stay hidden until it is authorized (HS2-1JT25R).
    expect(auth).not.toContain('data-component="grid"');
    for (const action of ['copy-github-code', 'reopen-github-sign-in', 'cancel-github-sign-in'])
      expect(auth).toContain(`data-action="${action}"`);
    const signedIn = String(
      ProviderSetupForm({
        kind: 'github',
        auth: { session: 's', userCode: '', verificationUri: '', state: 'authorized', repositories: ['a/b'] },
      }),
    );
    expect(signedIn).toContain('Signed in to GitHub.');
    // The responsive two-column form grid is app-owned (Kerf's Grid takes a fixed column count).
    expect(signedIn).toContain('<div class="provider-setup-form__grid">');
    expect(signedIn).toContain('<wa-input class="provider-setup-form__wide" name="connection-name"');
    expect(signedIn).toContain('<wa-checkbox class="provider-setup-form__wide" name="make-default"');
    expect(signedIn).not.toContain('name="api-base"');
    const remote = String(
      TicketSourceSetupDialog({
        project: { root: '/work/demo', name: 'Demo', stores: ['/work/demo.hs2'] },
        providerConnections: [],
        navigation: 'push',
        createdGitTicketStore: '/work/demo.hs2',
        remoteBusy: true,
        remoteError: 'Remote failed',
      }),
    );
    expect(remote).toContain('Back up this ticket repository');
    expect(remote).toContain('Connecting…');
    expect(remote).toContain('wa-progress-bar indeterminate');
    expect(remote).toContain('label="Connecting and pushing ticket repository"');
    expect(remote).toContain('Large repositories can take several minutes.');
    expect(remote).toContain('Remote failed');
    expect(providerName('jira')).toBe('Jira Cloud');
  });

  it('renders connected git and external sources with default metadata', () => {
    const markup = String(
      TicketSourcesSettings({
        stores: ['/work/demo.hs2'],
        providerConnections: [
          {
            id: 'github-main',
            provider: 'github',
            locator: 'small-tale/hotsheet2',
            name: 'Issues',
            default: true,
            settings: {},
          },
        ],
      }),
    );
    expect(markup).toContain('data-component="ticket-sources-settings"');
    expect(markup).toContain('1 git ticket source and 1 external provider');
    expect(markup).toContain('/work/demo.hs2');
    expect(markup).toContain('GitHub Issues');
    expect(markup).toContain('Default');
  });

  it('owns source and provider styles outside the global stylesheet', () => {
    const global = readFileSync(new URL('../style.css', import.meta.url), 'utf8');
    const provider = readFileSync(new URL('./provider-setup-form.css', import.meta.url), 'utf8');
    for (const selector of [
      'data-ticket-source-setup-dialog',
      '.ticket-source-setup',
      '.provider-setup-form',
      '.ticket-provider-settings',
    ])
      expect(global).not.toContain(selector);
    expect(readFileSync(new URL('./ticket-source-setup-dialog.css', import.meta.url), 'utf8')).toContain(
      'data-ticket-source-setup-dialog',
    );
    expect(provider).toContain('.provider-setup-form');
    expect(provider).toMatch(
      /@media \(max-width: remify\(768px\)\) \{\s*\.provider-setup-form__grid \{\s*grid-template-columns: 1fr;/,
    );
    expect(readFileSync(new URL('./ticket-sources-settings.css', import.meta.url), 'utf8')).toContain(
      '.ticket-provider-settings',
    );
  });
});
