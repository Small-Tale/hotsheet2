import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { providerName, ProviderSetupForm } from './provider-setup-form';
import { TicketSourceSetupDialog } from './ticket-source-setup-dialog';
import { AccountsSettings, TicketSourcesSettings } from './ticket-sources-settings';

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
    // A project disables its own source from its editor (HS2-SM9PM8).
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
      String(
        TicketSourcesSettings({
          sources: [
            {
              connectionId: 'github-main',
              name: 'Product issues',
              provider: 'github',
              locator: 'acme/repo',
              default: true,
              disabled,
            },
          ],
        }),
      );
    expect(settings(true)).toContain('<small data-state="disabled">Disabled</small>');
    expect(settings(false)).not.toContain('data-state="disabled"');
  });

  it('offers removal from this project only while editing, behind an inline confirmation (HS2-724S9N, HS2-SM9PM8)', () => {
    const connection = {
        id: 'github-main',
        provider: 'github',
        locator: 'acme/repo',
        name: 'Product issues',
        default: true,
        settings: {},
      },
      base = {
        project: { id: 'demo', root: '/work/demo', name: 'Demo', stores: [] },
        providerKind: 'github' as const,
        providerConnections: [connection],
        navigation: 'none' as const,
      };
    const connecting = String(TicketSourceSetupDialog(base));
    expect(connecting).not.toContain('request-provider-removal');
    const editing = String(TicketSourceSetupDialog({ ...base, editingProviderId: 'github-main' }));
    expect(editing).toContain('data-action="request-provider-removal"');
    expect(editing).toContain('Remove from this project…');
    expect(editing).toContain('data-lucide="trash-2"');
    expect(editing).toContain('data-action="submit-provider-setup"');
    expect(editing).not.toContain('confirm-provider-removal');
    const confirming = String(
      TicketSourceSetupDialog({ ...base, editingProviderId: 'github-main', removingProviderId: 'github-main' }),
    );
    expect(confirming).toContain('<strong>Remove Product issues from Demo?</strong>');
    expect(confirming).toContain('No other project uses it, so its connection is deleted.');
    expect(confirming).toContain(
      'Tickets stay in GitHub Issues, and your sign-in stays under App Settings → Accounts.',
    );
    // A source another project shares stays there.
    const shared = String(
      TicketSourceSetupDialog({
        ...base,
        providerConnections: [
          {
            ...connection,
            projects: [
              { id: 'demo', alias: 'Demo' },
              { id: 'site', alias: 'marketing-site' },
            ],
          },
        ],
        editingProviderId: 'github-main',
        removingProviderId: 'github-main',
      }),
    );
    expect(shared).toContain('It stays in marketing-site.');
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
        defaultChoice: true,
      }),
    );
    expect(signedIn).toContain('Signed in to GitHub.');
    // The two-column form grid is Kerf's responsive Grid (beta.62 `minColumnWidth`, HS2-7XX356).
    expect(signedIn).toMatch(
      /<div class="kui-grid"[^>]*data-min-column-width="true"[^>]*--_kui-grid-min-column-width:/,
    );
    expect(signedIn).not.toContain('provider-setup-form__grid');
    expect(signedIn).toContain('<wa-input class="provider-setup-form__wide" name="connection-name"');
    expect(signedIn).toContain('<wa-checkbox class="provider-setup-form__wide" name="make-default"');
    expect(signedIn).toContain('name="attachment-repo"');
    expect(signedIn).toContain('Enter a repository and save to enable attachments.');
    expect(signedIn).toContain('name="attachment-folder" label="Attachment folder" value="hotsheet-attachments"');
    expect(signedIn).toContain('name="attachment-branch" label="Attachment branch" value="main"');
    expect(signedIn).not.toContain('name="api-base"');
    const configured = String(
      ProviderSetupForm({
        kind: 'github',
        connection: {
          id: 'github-main',
          provider: 'github',
          locator: 'acme/issues',
          name: 'Issues',
          default: false,
          settings: { attachment_repo: 'acme/assets', attachment_folder: 'evidence', attachment_branch: 'media' },
        },
      }),
    );
    expect(configured).toContain('Currently enabled.');
    expect(configured).toContain('Attachment assets repository (optional)');
    expect(configured).toContain(
      'name="attachment-repo" autocomplete="off" placeholder="owner/repository" value="acme/assets"',
    );
    const listedEdit = String(
      ProviderSetupForm({
        kind: 'github',
        connection: {
          id: 'github-main',
          provider: 'github',
          locator: 'acme/issues',
          name: 'Issues',
          default: false,
          settings: { attachment_repo: 'acme/assets' },
        },
        auth: {
          session: 'edit:github-main',
          userCode: '',
          verificationUri: '',
          state: 'authorized',
          repositories: ['acme/issues', 'acme/assets'],
        },
      }),
    );
    expect(listedEdit).toContain('name="connection-locator" required list="provider-setup-github-repositories"');
    expect(listedEdit).toContain('name="attachment-repo" list="provider-setup-github-repositories"');
    expect(listedEdit).toContain('<option value="acme/assets"></option>');
    const failedEdit = String(
      ProviderSetupForm({
        kind: 'github',
        connection: {
          id: 'github-main',
          provider: 'github',
          locator: 'acme/issues',
          name: 'Issues',
          default: false,
          settings: { attachment_repo: 'acme/assets' },
        },
        auth: {
          session: 'edit:github-main',
          userCode: '',
          verificationUri: '',
          state: 'authorized',
          message: 'Access expired',
        },
      }),
    );
    expect(failedEdit).toContain('You can enter a repository path manually.');
    expect(failedEdit).toContain('value="acme/issues"');
    expect(failedEdit).not.toContain('provider-setup-github-repositories');
    expect(configured).toContain('name="attachment-folder" label="Attachment folder" value="evidence"');
    expect(configured).toContain('name="attachment-branch" label="Attachment branch" value="media"');
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

  it("lists only this project's own sources with its default and a remove action (HS2-3SCH1K, HS2-SM9PM8)", () => {
    const sources = [
        { connectionId: 'git-demo', name: 'Hot Sheet git', provider: 'git', locator: '/work/demo.hs2', default: false },
        {
          connectionId: 'github-main',
          name: 'Issues',
          provider: 'github',
          locator: 'small-tale/hotsheet2',
          default: true,
          sharedWith: ['marketing-site'],
        },
      ],
      markup = String(TicketSourcesSettings({ sources }));
    expect(markup).toContain('data-component="ticket-sources-settings"');
    expect(markup).toContain('This project uses 2 ticket sources.');
    expect(markup).toMatch(/name="project-default-source"[^>]*value="github-main"/);
    expect(markup).toContain('data-source-id="git-demo"');
    expect(markup).toContain('/work/demo.hs2');
    // The external row edits its source; its trailing action removes it from this project.
    expect(markup).toContain('data-action="edit-provider-connection"');
    expect(markup).toContain('data-action="remove-project-source"');
    expect(markup).toContain('aria-label="Remove Issues from this project"');
    expect(markup).toContain('data-lucide="unlink"');
    expect(markup).not.toContain('data-action="remove-project-source" data-source-id="git-demo"');
    expect(markup.match(/<small>Default<\/small>/g)).toHaveLength(1);
    // A source shared with another project (attached headlessly) says so.
    expect(markup).toContain('Also used by marketing-site');
    // Never another project's sources, and no machine-wide catalog to attach from.
    expect(markup).not.toContain('Other connections on this machine');
    expect(markup).not.toContain('attach-project-source');
    expect(markup).toContain('data-item-id="accounts"');
    expect(markup).toContain('App Settings → Accounts');
    const single = String(TicketSourcesSettings({ sources: sources.slice(0, 1) }));
    expect(single).toContain('This project uses 1 ticket source.');
    expect(single).not.toContain('project-default-source');
    expect(single).not.toContain('Also used by');
    expect(single.match(/<small>Default<\/small>/g)).toHaveLength(1);
  });

  it('lists machine-wide accounts with their sources and projects under App Settings → Accounts (HS2-SM9PM8)', () => {
    const accounts = [
        {
          id: 'github-app-01work',
          provider: 'github',
          host: 'github.com',
          managed: true,
          sources: [
            {
              connection_id: 'github-main',
              name: 'Issues',
              locator: 'acme/repo',
              disabled: true,
              projects: [
                { id: 'a', alias: 'procurement' },
                { id: 'b', alias: 'domotion' },
              ],
            },
          ],
          projects: [
            { id: 'b', alias: 'domotion' },
            { id: 'a', alias: 'procurement' },
          ],
        },
        { id: 'github-app-01unused', provider: 'github', host: '', managed: true, sources: [], projects: [] },
        {
          id: 'jira-token',
          provider: 'jira',
          host: 'acme.atlassian.net',
          identity: 'dev@acme.test',
          managed: false,
          sources: [{ connection_id: 'jira-ops', name: 'Ops', locator: 'OPS', disabled: false, projects: [] }],
          projects: [],
        },
      ],
      markup = String(AccountsSettings({ accounts }));
    expect(markup).toContain('data-component="accounts-settings"');
    expect(markup).toContain('data-account-id="github-app-01work"');
    expect(markup).toContain('acme/repo · Used by procurement, domotion');
    expect(markup).toContain('<small data-state="disabled">Disabled</small>');
    expect(markup).toContain('Signed in with Hot Sheet');
    expect(markup).toContain('dev@acme.test · Keychain credential jira-token');
    expect(markup).toContain('OPS · Not used by any project');
    expect(markup).toContain('Connection ID: jira-ops');
    expect(markup).toContain('data-action="request-unused-account-source-removal"');
    expect(markup).not.toContain('Connection ID: github-main');
    const confirming = String(AccountsSettings({ accounts, sourceRemovalChoice: 'jira-ops' }));
    expect(confirming).toContain('Remove this unused connection?');
    expect(confirming).toContain('data-action="remove-unused-account-source"');
    expect(confirming).toContain('data-account-id="jira-token" data-source-id="jira-ops"');
    // Only an account no source uses can be signed out, and its button names it.
    expect(markup.match(/data-action="sign-out-account"/g)).toHaveLength(1);
    expect(markup).toMatch(/data-action="sign-out-account" data-item-id="github-app-01unused"/);
    expect(markup).toContain('No ticket source uses this sign-in.');
    expect(markup).toContain('data-lucide="log-out"');
    const busy = String(AccountsSettings({ accounts, signingOut: 'github-app-01unused' }));
    expect(busy).toMatch(/data-item-id="github-app-01unused" disabled[\s\S]*?Signing out…/);
    const empty = String(AccountsSettings({ accounts: [], error: 'Server unavailable' }));
    expect(empty).toContain('No accounts yet.');
    expect(empty).toContain('role="alert"');
    expect(empty).toContain('Server unavailable');
    // Editing a source from its project offers Disable/Remove and this project's default choice.
    const connection = {
        id: 'github-main',
        provider: 'github',
        locator: 'acme/repo',
        name: 'Issues',
        default: false,
        settings: {},
      },
      dialog = (projectDefault: boolean) =>
        String(
          TicketSourceSetupDialog({
            project: { id: 'a', root: '/work/demo', name: 'Demo', stores: [] },
            providerKind: 'github',
            providerConnections: [connection],
            editingProviderId: 'github-main',
            projectDefault,
            navigation: 'none',
          }),
        );
    const editing = dialog(true);
    expect(editing).toContain('data-action="toggle-provider-disabled"');
    expect(editing).toContain('data-action="request-provider-removal"');
    expect(editing).toContain("Use as this project's default ticket source");
    expect(editing).toMatch(/name="make-default" value="on" checked/);
    expect(dialog(false)).not.toMatch(/name="make-default" value="on" checked/);
    expect(editing).not.toContain('ticket-source-setup__scope-hint');
  });

  it('offers signed-in GitHub accounts before a new sign-in (HS2-SM9PM8)', () => {
    const accounts = [
        {
          id: 'github-app-01work',
          provider: 'github',
          host: 'github.com',
          identity: 'alice',
          managed: true,
          sources: [],
          projects: [{ id: 'a', alias: 'procurement' }],
        },
        { id: 'jira-token', provider: 'jira', host: 'acme.atlassian.net', managed: false, sources: [], projects: [] },
      ],
      picker = String(ProviderSetupForm({ kind: 'github', accounts }));
    expect(picker).toContain('data-action="use-github-account"');
    expect(picker).toContain('data-item-id="github-app-01work"');
    expect(picker).toContain('Used by procurement');
    expect(picker).toContain('aria-label="Use alice on github.com, used by procurement"');
    expect(picker).toContain('Or sign in with another account.');
    expect(picker).toContain('data-action="start-github-sign-in"');
    // Only GitHub sign-ins are offered for GitHub, and none without accounts.
    expect(picker.match(/data-action="use-github-account"/g)).toHaveLength(1);
    const older = String(
      ProviderSetupForm({
        kind: 'github',
        accounts: [
          {
            id: 'github-app-01older',
            provider: 'github',
            host: 'github.com',
            managed: true,
            sources: [],
            projects: [],
          },
        ],
      }),
    );
    expect(older).toContain('github.com · sign-in 1older');
    const none = String(ProviderSetupForm({ kind: 'github' }));
    expect(none).not.toContain('use-github-account');
    expect(none).toContain('Sign in to choose a repository.');
    // Reusing an account says so instead of claiming a fresh sign-in.
    const reused = String(
      ProviderSetupForm({
        kind: 'github',
        accounts,
        auth: {
          session: 'account:github-app-01work',
          userCode: '',
          verificationUri: '',
          state: 'authorized',
          account: 'github-app-01work',
          credential: 'github-app-01work',
          repositories: ['acme/repo'],
        },
      }),
    );
    expect(reused).toContain('Using your GitHub account on GitHub.');
    expect(reused).not.toContain('use-github-account');
  });

  it('prechecks a new source only when this project has no default (HS2-VM6YG9)', () => {
    const auth = { session: 's', userCode: '', verificationUri: '', state: 'authorized' as const },
      render = (hasProjectDefault: boolean) =>
        String(
          TicketSourceSetupDialog({
            project: { id: 'demo', root: '/work/demo', name: 'Demo', stores: [] },
            providerKind: 'github',
            providerConnections: [],
            githubAuth: auth,
            hasProjectDefault,
            navigation: 'none',
          }),
        );
    expect(render(false)).toMatch(/name="make-default" value="on" checked/);
    expect(render(true)).toMatch(/name="make-default" value="on"(?! checked)/);
  });

  it('offers signed-in GitLab and Jira accounts and prefills a new source from the chosen one (HS2-F5HNJN)', () => {
    const accounts = [
        {
          id: 'jira-token',
          provider: 'jira',
          host: 'acme.atlassian.net',
          base_url: 'https://acme.atlassian.net',
          identity: 'dev@acme.test',
          managed: false,
          sources: [],
          projects: [{ id: 'a', alias: 'procurement' }],
        },
        {
          id: 'gitlab-corp',
          provider: 'gitlab',
          host: 'gitlab.corp.test',
          base_url: 'https://gitlab.corp.test/api/v4',
          managed: false,
          sources: [],
          projects: [],
        },
        { id: 'gitlab-dotcom', provider: 'gitlab', host: 'gitlab.com', managed: false, sources: [], projects: [] },
      ],
      inputValue = (markup: string, name: string) =>
        new RegExp(`<wa-input[^>]*name="${name}"[^>]*>`).exec(markup)?.[0].match(/ value="([^"]*)"/)?.[1] ?? '';
    // Before a choice: only this provider's accounts are offered and every field is blank.
    const jira = String(ProviderSetupForm({ kind: 'jira', accounts }));
    expect(jira.match(/data-action="use-provider-account"/g)).toHaveLength(1);
    expect(jira).toContain('data-item-id="jira-token"');
    expect(jira).toContain(
      'aria-label="Use the Jira Cloud account dev@acme.test on acme.atlassian.net, used by procurement"',
    );
    expect(jira).toContain('This project still enters its own project key.');
    expect(jira).not.toContain('aria-pressed="true"');
    for (const field of ['credential-reference', 'jira-email', 'api-base']) expect(inputValue(jira, field)).toBe('');
    // The chosen account prefills its credential, email, and site; the project key stays empty.
    const chosen = String(ProviderSetupForm({ kind: 'jira', accounts, chosenAccount: 'jira-token' }));
    expect(chosen).toContain('aria-pressed="true"');
    expect(inputValue(chosen, 'credential-reference')).toBe('jira-token');
    expect(inputValue(chosen, 'jira-email')).toBe('dev@acme.test');
    expect(inputValue(chosen, 'api-base')).toBe('https://acme.atlassian.net');
    expect(inputValue(chosen, 'connection-locator')).toBe('');
    expect(chosen).toContain('data-key="credential-jira-token"');
    // GitLab: a self-managed account fills its API base; a gitlab.com one leaves the default.
    const gitlab = String(ProviderSetupForm({ kind: 'gitlab', accounts, chosenAccount: 'gitlab-corp' }));
    expect(gitlab.match(/data-action="use-provider-account"/g)).toHaveLength(2);
    expect(gitlab).toContain('This project still enters its own project path.');
    expect(inputValue(gitlab, 'credential-reference')).toBe('gitlab-corp');
    expect(inputValue(gitlab, 'api-base')).toBe('https://gitlab.corp.test/api/v4');
    const dotcom = String(ProviderSetupForm({ kind: 'gitlab', accounts, chosenAccount: 'gitlab-dotcom' }));
    expect(inputValue(dotcom, 'credential-reference')).toBe('gitlab-dotcom');
    expect(inputValue(dotcom, 'api-base')).toBe('');
    // A choice for another provider is ignored, and editing never offers or applies accounts.
    expect(
      inputValue(
        String(ProviderSetupForm({ kind: 'gitlab', accounts, chosenAccount: 'jira-token' })),
        'credential-reference',
      ),
    ).toBe('');
    const editing = String(
      ProviderSetupForm({
        kind: 'jira',
        accounts,
        chosenAccount: 'jira-token',
        connection: {
          id: 'jira-eng',
          provider: 'jira',
          locator: 'ENG',
          name: 'Eng',
          default: false,
          settings: {
            credential: { secret: 'other-token' },
            email: 'ops@acme.test',
            base_url: 'https://ops.atlassian.net',
          },
        },
      }),
    );
    expect(editing).not.toContain('use-provider-account');
    expect(inputValue(editing, 'credential-reference')).toBe('other-token');
    expect(inputValue(editing, 'jira-email')).toBe('ops@acme.test');
    // Without accounts there is no picker.
    expect(String(ProviderSetupForm({ kind: 'gitlab' }))).not.toContain('use-provider-account');
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
    // The collapse to one column is Kerf's (responsive Grid); the app keeps only the full-width rows
    // and the form's maximum width.
    expect(provider).not.toContain('provider-setup-form__grid');
    expect(provider).toMatch(/\.provider-setup-form \{[^}]*max-width: remify\(640px\)/);
    expect(provider).toMatch(/\.provider-setup-form__wide \{[^}]*grid-column: 1 \/ -1/);
    expect(readFileSync(new URL('./ticket-sources-settings.css', import.meta.url), 'utf8')).toContain(
      '.ticket-provider-settings',
    );
  });
});
