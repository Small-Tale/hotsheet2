import './provider-setup-form.css';

import { Button } from '@kerfjs/ui/button';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { Combobox } from '@kerfjs/ui/combobox';
import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { SunkenPanel } from '@kerfjs/ui/sunken-panel';
import { Text } from '@kerfjs/ui/text';
import type { AttrSpec } from 'kerfjs';
import { Check, ChevronRight, Copy, ExternalLink, LogIn, RefreshCw } from 'lucide';

import type { ProviderAccount, ProviderConnection } from '../api';
import { DEFAULT_GITHUB_ATTACHMENT_BRANCH, DEFAULT_GITHUB_ATTACHMENT_FOLDER } from '../github-attachment-settings';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { FlowBackButton } from './flow-back-button';
import { ProviderIcon } from './provider-icon';
import { TicketSourceColorPicker } from './ticket-source-color-picker';

export type ExternalProviderKind = 'github' | 'gitlab' | 'jira';

export interface GithubAuthState {
  session: string;
  userCode: string;
  verificationUri: string;
  /** `idle` before sign-in starts (for example after choosing GitHub Enterprise). */
  state: 'idle' | 'waiting' | 'authorized' | 'denied' | 'expired' | 'cancelled' | 'error';
  /** Signing in to GitHub Enterprise: its server URL is asked for before sign-in (HS2-1JT25R). */
  enterprise?: boolean;
  enterpriseUrl?: string;
  /** Whether the one-time code reached the clipboard automatically. */
  copied?: boolean;
  credential?: string;
  /** Reusing this machine-wide account instead of a new sign-in (HS2-SM9PM8). */
  account?: string;
  repositories?: string[];
  /** What each GitHub App installation grants, to explain a missing repository (HS2-27T5WT). */
  installations?: Array<{ account: string; selection: string; settingsUrl?: string }>;
  installUrl?: string;
  /** Re-listing repositories after the user changed the app's access on GitHub. */
  refreshing?: boolean;
  message?: string;
}

export const providerName = (kind: ExternalProviderKind) =>
  ({ github: 'GitHub Issues', gitlab: 'GitLab Issues', jira: 'Jira Cloud' })[kind];

function connectionSetting(connection: ProviderConnection | undefined, key: string) {
  const value = connection?.settings[key];
  return typeof value === 'string' ? value : '';
}
function connectionCredential(connection?: ProviderConnection) {
  if (!connection) return '';
  const credential = connection.settings.credential;
  return credential && typeof credential === 'object' && 'secret' in credential && typeof credential.secret === 'string'
    ? credential.secret
    : '';
}

export interface ProviderSetupFormProps {
  kind: ExternalProviderKind;
  connection?: ProviderConnection;
  auth?: GithubAuthState;
  error?: string;
  /**
   * The "this project's default ticket source" checkbox state, or `undefined` to omit it (as the UX
   * demo does for a bare form).
   */
  defaultChoice?: boolean;
  /**
   * Machine-wide sign-ins a new source can reuse instead of signing in again: GitHub accounts before a
   * new sign-in (HS2-SM9PM8), GitLab and Jira accounts above the form (HS2-F5HNJN).
   */
  accounts?: readonly ProviderAccount[];
  /** The GitLab or Jira account whose credential, email, and site prefill a new source (HS2-F5HNJN). */
  chosenAccount?: string;
  /** This checkout's mark color for the connection being edited. */
  sourceColor?: string;
}

/** What identifies a GitLab or Jira account in the picker: its host, plus the Jira email. */
function accountTitle(account: ProviderAccount) {
  return account.identity ? `${account.identity} on ${account.host || account.id}` : account.host || account.id;
}

function githubAccountTitle(account: ProviderAccount) {
  if (account.identity) return `${account.identity} on ${account.host || 'GitHub'}`;
  const host = account.host || 'GitHub';
  return `${host} · sign-in ${account.id.slice(-6)}`;
}

function accountUsage(account: ProviderAccount, capitalized = true) {
  const usage = account.projects.length
    ? `used by ${account.projects.map((project) => project.alias).join(', ')}`
    : 'not used by any project yet';
  return capitalized ? usage.charAt(0).toUpperCase() + usage.slice(1) : usage;
}

/**
 * The "Ticket source types" back control at the top of a ticket-source setup screen. The provider
 * form and the ticket-source setup dialog's other screens (the remote backup form) compose it
 * with their own back action (HS2-WP69TD).
 */
export function ProviderSetupBackButton({ action }: { action: AttrSpec<'data-action'> }) {
  // The one chevron back affordance shared by multi-screen setup dialogs (HS2-WJ4JDW).
  return <FlowBackButton action={action.value} label="Ticket source types" />;
}

export function ProviderSetupForm({
  kind,
  connection,
  auth,
  error = '',
  defaultChoice,
  accounts = [],
  chosenAccount,
  sourceColor,
}: ProviderSetupFormProps) {
  const labels = {
      github: ['GitHub Issues', 'owner/repository', 'GitHub credential reference'],
      gitlab: ['GitLab Issues', 'namespace/project', 'GitLab credential reference'],
      jira: ['Jira Cloud', 'PROJECT', 'Jira API-token reference'],
    }[kind],
    editing = Boolean(connection),
    apiBase = connectionSetting(connection, kind === 'jira' ? 'base_url' : 'api_base'),
    attachmentRepo = connectionSetting(connection, 'attachment_repo'),
    repositories = auth?.repositories ?? [],
    signedIn = auth?.state === 'authorized',
    // A new GitHub connection starts from sign-in; its settings appear once GitHub has authorized it.
    showFields = kind !== 'github' || editing || signedIn,
    choosing = kind === 'github' && signedIn && auth.repositories !== undefined,
    limited = (auth?.installations ?? []).filter((installation) => installation.selection !== 'all'),
    githubAccounts = accounts.filter((account) => account.provider === 'github'),
    // A new GitLab or Jira source can start from an account signed in on this computer (HS2-F5HNJN).
    reusable = kind !== 'github' && !editing ? accounts.filter((account) => account.provider === kind) : [],
    chosen = reusable.find((account) => account.id === chosenAccount),
    // Prefilled controls are keyed by the chosen account so a choice replaces them with its values.
    prefillKey = chosen?.id ?? 'manual',
    credentialValue = chosen ? chosen.id : connectionCredential(connection),
    emailValue = chosen ? (chosen.identity ?? '') : connectionSetting(connection, 'email'),
    apiBaseValue = chosen ? (chosen.base_url ?? '') : apiBase;
  return (
    <form
      id="provider-setup-form"
      class="provider-setup-form"
      data-component="provider-setup-form"
      {...COMMANDS_AND_AI_ACTIONS.saveProviderConnection.attrs}
    >
      <ProviderSetupBackButton action={COMMANDS_AND_AI_ACTIONS.backProviderKind} />
      {/* Buttons carry `data-key` so a re-render never recycles the clicked control into a different
          action while that click is still dispatching (for example Enterprise ↔ GitHub.com; KF-HK7WE8). */}
      {kind === 'github' && !editing && (
        <SunkenPanel className="provider-setup-form__github-auth" ariaLabel="GitHub Sign In">
          {auth?.state === 'waiting' ? (
            <>
              <Text flush>
                Paste this one-time code in the GitHub window
                {auth.copied ? ' — it is already on your clipboard' : ''}, then approve Hot Sheet and close that window.
              </Text>
              <output class="provider-setup-form__device-code" aria-label="One-time GitHub code">
                {auth.userCode}
              </output>
              <Row vAlign="middle" gap="xs" wrap>
                <Button
                  emphasis="primary"
                  type="button"
                  action={COMMANDS_AND_AI_ACTIONS.copyGithubCode.value}
                  rootAttributes={{ 'data-key': 'copy-github-code' }}
                  icon={<LucideIcon size="s" icon={Copy} name="copy" />}
                >
                  {auth.copied ? 'Copy again' : 'Copy code'}
                </Button>
                <Button
                  emphasis="plain"
                  type="button"
                  action={COMMANDS_AND_AI_ACTIONS.reopenGithubSignIn.value}
                  rootAttributes={{ 'data-key': 'reopen-github-sign-in' }}
                  icon={<LucideIcon size="s" icon={ExternalLink} name="external-link" />}
                >
                  Reopen GitHub
                </Button>
                <Button
                  emphasis="plain"
                  type="button"
                  action={COMMANDS_AND_AI_ACTIONS.cancelGithubSignIn.value}
                  rootAttributes={{ 'data-key': 'cancel-github-sign-in' }}
                >
                  Cancel
                </Button>
              </Row>
              <Text tone="quiet" flush role="status">
                Waiting for GitHub…
              </Text>
            </>
          ) : signedIn ? (
            <Text flush role="status">
              {auth.account ? 'Using your GitHub account on ' : 'Signed in to '}
              {auth.enterpriseUrl ? new URL(auth.enterpriseUrl).host : 'GitHub'}.{' '}
              {auth.repositories ? 'Choose a repository for this project below.' : 'Loading your repositories…'}
            </Text>
          ) : auth?.enterprise ? (
            <>
              <wa-input
                name="github-enterprise-url"
                type="url"
                label="GitHub Enterprise Server URL"
                required
                placeholder="https://github.example.com"
                value={auth.enterpriseUrl ?? ''}
              >
                <span slot="hint">The address you use to open GitHub Enterprise in a browser.</span>
              </wa-input>
              <Row vAlign="middle" gap="xs" wrap>
                <Button
                  emphasis="primary"
                  type="button"
                  action={COMMANDS_AND_AI_ACTIONS.startGithubSignIn.value}
                  rootAttributes={{ 'data-key': 'start-github-sign-in' }}
                  icon={<LucideIcon size="s" icon={LogIn} name="log-in" />}
                >
                  Sign in with GitHub Enterprise
                </Button>
                <Button
                  emphasis="plain"
                  type="button"
                  action={COMMANDS_AND_AI_ACTIONS.chooseGithubDotcom.value}
                  rootAttributes={{ 'data-key': 'choose-github-dotcom' }}
                >
                  Use GitHub.com Instead
                </Button>
              </Row>
            </>
          ) : (
            <>
              {githubAccounts.length > 0 && (
                <>
                  <Text flush>
                    Use a GitHub account already signed in on this computer, then choose this project's repository.
                  </Text>
                  <div class="provider-setup-form__accounts">
                    <List>
                      {githubAccounts.map((account, index) => {
                        const shared = {
                          action: 'use-github-account',
                          itemId: account.id,
                          multiline: true,
                          divider: index > 0 ? ('before' as const) : ('none' as const),
                          accessibleLabel: `Use ${githubAccountTitle(account)}, ${accountUsage(account, false)}`,
                          icon: <ProviderIcon kind="github" />,
                          label: (
                            <span class="provider-setup-form__account-copy">
                              <strong>{githubAccountTitle(account)}</strong>
                              <Text variant="span" tone="quiet" size="compact">
                                {accountUsage(account)}
                              </Text>
                            </span>
                          ),
                        };
                        // Usernames are fetched automatically; a row never needs a lookup action (HS2-NRTH1N).
                        return (
                          <ListItem
                            {...shared}
                            trailing={<LucideIcon icon={ChevronRight} name="chevron-right" size={16} />}
                          />
                        );
                      })}
                    </List>
                  </div>
                </>
              )}
              <Text flush>
                {githubAccounts.length ? 'Or sign in with another account. ' : 'Sign in to choose a repository. '}Hot
                Sheet opens GitHub in a small window and copies your one-time code for you.
              </Text>
              <Row vAlign="middle" gap="xs" wrap>
                <Button
                  emphasis="primary"
                  type="button"
                  action={COMMANDS_AND_AI_ACTIONS.startGithubSignIn.value}
                  rootAttributes={{ 'data-key': 'start-github-sign-in' }}
                  icon={<LucideIcon size="s" icon={LogIn} name="log-in" />}
                >
                  Sign in with GitHub
                </Button>
                <Button
                  emphasis="plain"
                  type="button"
                  action={COMMANDS_AND_AI_ACTIONS.chooseGithubEnterprise.value}
                  rootAttributes={{ 'data-key': 'choose-github-enterprise' }}
                >
                  Use GitHub Enterprise…
                </Button>
              </Row>
            </>
          )}
          {auth && ['denied', 'expired', 'cancelled', 'error'].includes(auth.state) && (
            <Text tone="danger" flush role="alert">
              {auth.message ?? `GitHub sign in was ${auth.state}. Try again.`}
            </Text>
          )}
          {auth?.message && !['denied', 'expired', 'cancelled', 'error'].includes(auth.state) ? (
            <Text tone="danger" flush role="alert">
              {auth.message}
            </Text>
          ) : (
            <></>
          )}
        </SunkenPanel>
      )}
      {reusable.length > 0 && (
        <SunkenPanel
          className="provider-setup-form__account-reuse"
          ariaLabel={`Signed-In ${providerName(kind)} Accounts`}
        >
          <Text flush>
            Use an account already signed in on this computer to fill in its credential
            {kind === 'jira' ? ', email, and site' : ' and server'}. This project still enters its own{' '}
            {kind === 'jira' ? 'project key' : 'project path'}.
          </Text>
          <div class="provider-setup-form__accounts">
            <List>
              {reusable.map((account, index) => (
                <ListItem
                  action="use-provider-account"
                  itemId={account.id}
                  multiline
                  pressed={account.id === chosen?.id}
                  divider={index > 0 ? 'before' : 'none'}
                  accessibleLabel={`Use the ${providerName(kind)} account ${accountTitle(account)}, ${accountUsage(account, false)}`}
                  icon={<ProviderIcon kind={kind} />}
                  trailing={
                    account.id === chosen?.id ? (
                      <LucideIcon icon={Check} name="check" size={16} />
                    ) : (
                      <LucideIcon icon={ChevronRight} name="chevron-right" size={16} />
                    )
                  }
                  label={
                    <span class="provider-setup-form__account-copy">
                      <strong>{accountTitle(account)}</strong>
                      <Text variant="span" tone="quiet" size="compact">
                        {accountUsage(account)}
                      </Text>
                    </span>
                  }
                />
              ))}
            </List>
          </div>
        </SunkenPanel>
      )}
      {/* Two 15rem-minimum columns that collapse to one when the form is narrower (Kerf beta.62
          responsive Grid, HS2-7XX356); the form's max width keeps a third column from appearing. */}
      {showFields && (
        <Grid minColumnWidth={rem(15)} gap="m">
          <wa-input
            class="provider-setup-form__wide"
            name="connection-name"
            label="Display Name"
            placeholder={labels[0]}
            value={connection?.name ?? ''}
          >
            <span slot="hint">Leave blank to use “{labels[0]}”.</span>
          </wa-input>
          {choosing ? (
            // Kerf Combobox filters every reachable repository as the person types and still accepts a
            // typed one (HS2-27T5WT, HS2-EN1E76).
            <div class="provider-setup-form__wide">
              <Combobox
                id="provider-setup-repository"
                name="connection-locator"
                label="Repository"
                placeholder="Search your repositories…"
                value={connection?.locator ?? ''}
                suggestions={repositories}
                createLabel="Use “{value}”"
                hint={`${repositories.length === 1 ? '1 repository' : `${repositories.length} repositories`} available. Type to filter.`}
              />
            </div>
          ) : (
            <wa-input
              class="provider-setup-form__wide"
              name="connection-locator"
              label={kind === 'jira' ? 'Project key' : 'Repository'}
              required
              placeholder={labels[1]}
              value={connection?.locator ?? ''}
            />
          )}
          {choosing && (
            <section class="provider-setup-form__wide" aria-label="GitHub Repository Access">
              <SunkenPanel>
                {auth.installations?.length ? (
                  <Text size="compact" tone="quiet" flush>
                    Missing a repository? GitHub only shows repositories the Hot Sheet app is installed on
                    {limited.length ? (
                      <>
                        , and it can see only the repositories you chose on{' '}
                        {limited.map((installation, index) => (
                          <>
                            {index > 0 && ', '}
                            <strong>{installation.account}</strong>
                          </>
                        ))}
                      </>
                    ) : (
                      ''
                    )}
                    .
                  </Text>
                ) : (
                  <Text size="compact" tone="quiet" flush>
                    The Hot Sheet GitHub App is not installed on any account yet, so no repositories are available.
                    Install it on GitHub, then refresh this list.
                  </Text>
                )}
                <Row vAlign="middle" gap={{ column: 'm', row: 'xs' }} wrap>
                  {limited
                    .filter((installation) => installation.settingsUrl)
                    .map((installation) => (
                      <a
                        class="provider-setup-form__repository-link"
                        href={installation.settingsUrl}
                        target="_blank"
                        rel="noopener"
                      >
                        Change access for {installation.account}{' '}
                        <LucideIcon size={14} icon={ExternalLink} name="external-link" />
                      </a>
                    ))}
                  {auth.installUrl ? (
                    <a
                      class="provider-setup-form__repository-link"
                      href={auth.installUrl}
                      target="_blank"
                      rel="noopener"
                    >
                      Add another account or organization{' '}
                      <LucideIcon size={14} icon={ExternalLink} name="external-link" />
                    </a>
                  ) : (
                    <></>
                  )}
                  <button
                    class="provider-setup-form__repository-link"
                    type="button"
                    {...COMMANDS_AND_AI_ACTIONS.refreshGithubRepositories.attrs}
                    data-key="refresh-github-repositories"
                    disabled={auth.refreshing}
                  >
                    <LucideIcon size={14} icon={RefreshCw} name="refresh-cw" />{' '}
                    {auth.refreshing ? 'Refreshing…' : 'Refresh list'}
                  </button>
                </Row>
              </SunkenPanel>
            </section>
          )}
          {kind === 'github' && (
            <>
              <div class="provider-setup-form__wide">
                <Combobox
                  id="provider-setup-attachment-repo"
                  name="attachment-repo"
                  label="Attachment Assets Repository (Optional)"
                  placeholder="owner/repository"
                  value={attachmentRepo}
                  suggestions={choosing ? repositories : []}
                  createLabel="Use “{value}”"
                  hint={`${
                    attachmentRepo
                      ? 'Assets repository configured. Clear this field and save to remove it. '
                      : 'Enter an assets repository and save to configure attachments. '
                  }The Hot Sheet GitHub App needs Contents (read and write) permission and access to this repository. Its installation owner must approve new permissions before uploads work. Files are committed here and linked in issues.`}
                />
              </div>
              <wa-input
                name="attachment-folder"
                label="Attachment Folder"
                value={connectionSetting(connection, 'attachment_folder') || DEFAULT_GITHUB_ATTACHMENT_FOLDER}
              >
                <span slot="hint">Relative folder in the assets repository.</span>
              </wa-input>
              <wa-input
                name="attachment-branch"
                label="Attachment Branch"
                value={connectionSetting(connection, 'attachment_branch') || DEFAULT_GITHUB_ATTACHMENT_BRANCH}
              >
                <span slot="hint">Existing branch to receive attachment files.</span>
              </wa-input>
            </>
          )}
          {/* GitHub uses the credential saved by Sign in with GitHub; there is no manual reference (HS2-48GA17). */}
          {kind !== 'github' && (
            <wa-input
              class="provider-setup-form__wide"
              name="credential-reference"
              label="Credential Reference"
              required
              placeholder={labels[2]}
              value={credentialValue}
              data-key={`credential-${prefillKey}`}
            >
              <span slot="hint">
                Create this reference first with <code>hotsheet key set</code>; the token is never returned to the
                browser.
              </span>
            </wa-input>
          )}
          {kind === 'jira' && (
            <>
              <wa-input
                name="jira-email"
                type="email"
                label="Account Email"
                required
                value={emailValue}
                data-key={`email-${prefillKey}`}
              />
              <wa-input
                name="api-base"
                type="url"
                label="Jira Site URL"
                required
                placeholder="https://company.atlassian.net"
                value={apiBaseValue}
                data-key={`api-base-${prefillKey}`}
              />
            </>
          )}
          {kind === 'gitlab' && (
            <wa-input
              class="provider-setup-form__wide"
              name="api-base"
              type="url"
              label="API Base URL (Optional)"
              placeholder="https://gitlab.com/api/v4"
              value={apiBaseValue}
              data-key={`api-base-${prefillKey}`}
            />
          )}
          {defaultChoice !== undefined && (
            <div class="provider-setup-form__wide">
              <Checkbox
                name="make-default"
                value="on"
                checked={defaultChoice}
                label="Use as this project's default ticket source"
                hint="New tickets in this project will be created here."
              />
            </div>
          )}
        </Grid>
      )}
      {editing && (
        <List gap="xs">
          <TicketSourceColorPicker
            source={{ provider: kind, name: connection?.name ?? providerName(kind), color: sourceColor }}
          />
          <Text variant="span" tone="quiet" size="compact">
            This icon color applies only to this project.
          </Text>
        </List>
      )}
      {error && (
        <Text tone="danger" flush role="alert">
          {error}
        </Text>
      )}
      {editing && auth?.message && (
        <Text tone="danger" flush role="alert">
          Could not load GitHub repositories: {auth.message}. You can enter a repository path manually.
        </Text>
      )}
    </form>
  );
}
