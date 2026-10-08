import './provider-setup-form.css';
import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';

import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { SunkenPanel } from '@kerfjs/ui/sunken-panel';
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
        <SunkenPanel className="provider-setup-form__github-auth" ariaLabel="GitHub sign in">
          {auth?.state === 'waiting' ? (
            <>
              <p class="provider-setup-form__auth-copy">
                Paste this one-time code in the GitHub window
                {auth.copied ? ' — it is already on your clipboard' : ''}, then approve Hot Sheet and close that window.
              </p>
              <output class="provider-setup-form__device-code" aria-label="One-time GitHub code">
                {auth.userCode}
              </output>
              <div class="provider-setup-form__auth-actions">
                <wa-button
                  appearance="accent"
                  type="button"
                  {...COMMANDS_AND_AI_ACTIONS.copyGithubCode.attrs}
                  data-key="copy-github-code"
                >
                  <LucideIcon size="s" slot="start" icon={Copy} name="copy" />{' '}
                  {auth.copied ? 'Copy again' : 'Copy code'}
                </wa-button>
                <wa-button
                  appearance="plain"
                  type="button"
                  {...COMMANDS_AND_AI_ACTIONS.reopenGithubSignIn.attrs}
                  data-key="reopen-github-sign-in"
                >
                  <LucideIcon size="s" slot="start" icon={ExternalLink} name="external-link" /> Reopen GitHub
                </wa-button>
                <wa-button
                  appearance="plain"
                  type="button"
                  {...COMMANDS_AND_AI_ACTIONS.cancelGithubSignIn.attrs}
                  data-key="cancel-github-sign-in"
                >
                  Cancel
                </wa-button>
              </div>
              <p class="provider-setup-form__auth-copy provider-setup-form__auth-waiting" role="status">
                Waiting for GitHub…
              </p>
            </>
          ) : signedIn ? (
            <p class="provider-setup-form__auth-copy" role="status">
              {auth.account ? 'Using your GitHub account on ' : 'Signed in to '}
              {auth.enterpriseUrl ? new URL(auth.enterpriseUrl).host : 'GitHub'}.{' '}
              {auth.repositories ? 'Choose a repository for this project below.' : 'Loading your repositories…'}
            </p>
          ) : auth?.enterprise ? (
            <>
              <wa-input
                name="github-enterprise-url"
                type="url"
                label="GitHub Enterprise server URL"
                required
                placeholder="https://github.example.com"
                value={auth.enterpriseUrl ?? ''}
              >
                <span slot="hint">The address you use to open GitHub Enterprise in a browser.</span>
              </wa-input>
              <div class="provider-setup-form__auth-actions">
                <wa-button
                  appearance="accent"
                  type="button"
                  {...COMMANDS_AND_AI_ACTIONS.startGithubSignIn.attrs}
                  data-key="start-github-sign-in"
                >
                  <LucideIcon size="s" slot="start" icon={LogIn} name="log-in" /> Sign in with GitHub Enterprise
                </wa-button>
                <wa-button
                  appearance="plain"
                  type="button"
                  {...COMMANDS_AND_AI_ACTIONS.chooseGithubDotcom.attrs}
                  data-key="choose-github-dotcom"
                >
                  Use GitHub.com instead
                </wa-button>
              </div>
            </>
          ) : (
            <>
              {githubAccounts.length > 0 && (
                <>
                  <p class="provider-setup-form__auth-copy">
                    Use a GitHub account already signed in on this computer, then choose this project's repository.
                  </p>
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
                              <small class="provider-setup-form__account-usage">{accountUsage(account)}</small>
                            </span>
                          ),
                        };
                        return account.identity ? (
                          <ListItem
                            {...shared}
                            trailing={<LucideIcon icon={ChevronRight} name="chevron-right" size={16} />}
                          />
                        ) : (
                          <ListActionRow
                            {...shared}
                            trailingAction="identify-github-account"
                            trailingActionLabel={`Show username for ${account.host || 'GitHub'} sign-in ${account.id.slice(-6)}`}
                            trailingActionTitle="Show username"
                            trailingActionIcon={<LucideIcon icon={RefreshCw} name="refresh-cw" />}
                          />
                        );
                      })}
                    </List>
                  </div>
                </>
              )}
              <p class="provider-setup-form__auth-copy">
                {githubAccounts.length ? 'Or sign in with another account. ' : 'Sign in to choose a repository. '}Hot
                Sheet opens GitHub in a small window and copies your one-time code for you.
              </p>
              <div class="provider-setup-form__auth-actions">
                <wa-button
                  appearance="accent"
                  type="button"
                  {...COMMANDS_AND_AI_ACTIONS.startGithubSignIn.attrs}
                  data-key="start-github-sign-in"
                >
                  <LucideIcon size="s" slot="start" icon={LogIn} name="log-in" /> Sign in with GitHub
                </wa-button>
                <wa-button
                  appearance="plain"
                  type="button"
                  {...COMMANDS_AND_AI_ACTIONS.chooseGithubEnterprise.attrs}
                  data-key="choose-github-enterprise"
                >
                  Use GitHub Enterprise…
                </wa-button>
              </div>
            </>
          )}
          {auth && ['denied', 'expired', 'cancelled', 'error'].includes(auth.state) && (
            <p class="provider-setup-form__auth-copy" role="alert">
              {auth.message ?? `GitHub sign in was ${auth.state}. Try again.`}
            </p>
          )}
          {auth?.message && !['denied', 'expired', 'cancelled', 'error'].includes(auth.state) ? (
            <p class="provider-setup-form__auth-copy" role="alert">
              {auth.message}
            </p>
          ) : (
            <></>
          )}
        </SunkenPanel>
      )}
      {reusable.length > 0 && (
        <SunkenPanel
          className="provider-setup-form__account-reuse"
          ariaLabel={`Signed-in ${providerName(kind)} accounts`}
        >
          <p class="provider-setup-form__auth-copy">
            Use an account already signed in on this computer to fill in its credential
            {kind === 'jira' ? ', email, and site' : ' and server'}. This project still enters its own{' '}
            {kind === 'jira' ? 'project key' : 'project path'}.
          </p>
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
                      <small class="provider-setup-form__account-usage">{accountUsage(account)}</small>
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
            label="Display name"
            placeholder={labels[0]}
            value={connection?.name ?? ''}
          >
            <span slot="hint">Leave blank to use “{labels[0]}”.</span>
          </wa-input>
          {choosing ? (
            <label class="provider-setup-form__wide provider-setup-form__field">
              Repository
              {/* Native datalist keeps browser search over every reachable repository (HS2-27T5WT). */}
              <input
                class="provider-setup-form__field-control"
                name="connection-locator"
                required
                list="provider-setup-github-repositories"
                autocomplete="off"
                placeholder="Search your repositories…"
                value={connection?.locator ?? ''}
              />
              <datalist id="provider-setup-github-repositories">
                {repositories.map((repository) => (
                  <option value={repository} />
                ))}
              </datalist>
              <small class="provider-setup-form__field-hint">
                {repositories.length === 1 ? '1 repository' : `${repositories.length} repositories`} available. Type to
                filter.
              </small>
            </label>
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
            <section
              class="provider-setup-form__wide provider-setup-form__repository-access"
              aria-label="GitHub repository access"
            >
              {auth.installations?.length ? (
                <p>
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
                </p>
              ) : (
                <p>
                  The Hot Sheet GitHub App is not installed on any account yet, so no repositories are available.
                  Install it on GitHub, then refresh this list.
                </p>
              )}
              <div class="provider-setup-form__repository-actions">
                {limited
                  .filter((installation) => installation.settingsUrl)
                  .map((installation) => (
                    <a href={installation.settingsUrl} target="_blank" rel="noopener">
                      Change access for {installation.account}{' '}
                      <LucideIcon size={14} icon={ExternalLink} name="external-link" />
                    </a>
                  ))}
                {auth.installUrl && (
                  <a href={auth.installUrl} target="_blank" rel="noopener">
                    Add another account or organization{' '}
                    <LucideIcon size={14} icon={ExternalLink} name="external-link" />
                  </a>
                )}
                <button
                  type="button"
                  {...COMMANDS_AND_AI_ACTIONS.refreshGithubRepositories.attrs}
                  data-key="refresh-github-repositories"
                  disabled={auth.refreshing}
                >
                  <LucideIcon size={14} icon={RefreshCw} name="refresh-cw" />{' '}
                  {auth.refreshing ? 'Refreshing…' : 'Refresh list'}
                </button>
              </div>
            </section>
          )}
          {kind === 'github' && (
            <>
              <label class="provider-setup-form__wide provider-setup-form__field">
                Attachment assets repository (optional)
                <input
                  class="provider-setup-form__field-control"
                  name="attachment-repo"
                  list={choosing ? 'provider-setup-github-repositories' : undefined}
                  autocomplete="off"
                  placeholder="owner/repository"
                  value={attachmentRepo}
                />
                <small class="provider-setup-form__field-hint">
                  {attachmentRepo
                    ? 'Assets repository configured. Clear this field and save to remove it. '
                    : 'Enter an assets repository and save to configure attachments. '}
                  The Hot Sheet GitHub App needs Contents (read and write) permission and access to this repository. Its
                  installation owner must approve new permissions before uploads work. Files are committed here and
                  linked in issues.
                </small>
              </label>
              <wa-input
                name="attachment-folder"
                label="Attachment folder"
                value={connectionSetting(connection, 'attachment_folder') || DEFAULT_GITHUB_ATTACHMENT_FOLDER}
              >
                <span slot="hint">Relative folder in the assets repository.</span>
              </wa-input>
              <wa-input
                name="attachment-branch"
                label="Attachment branch"
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
              label="Credential reference"
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
                label="Account email"
                required
                value={emailValue}
                data-key={`email-${prefillKey}`}
              />
              <wa-input
                name="api-base"
                type="url"
                label="Jira site URL"
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
              label="API base URL (optional)"
              placeholder="https://gitlab.com/api/v4"
              value={apiBaseValue}
              data-key={`api-base-${prefillKey}`}
            />
          )}
          {defaultChoice !== undefined && (
            <wa-checkbox class="provider-setup-form__wide" name="make-default" value="on" checked={defaultChoice}>
              Use as this project's default ticket source
              <span slot="hint">New tickets in this project will be created here.</span>
            </wa-checkbox>
          )}
        </Grid>
      )}
      {editing && (
        <div class="provider-setup-form__source-color">
          <TicketSourceColorPicker
            source={{ provider: kind, name: connection?.name ?? providerName(kind), color: sourceColor }}
          />
          <small>This icon color applies only to this project.</small>
        </div>
      )}
      {error && (
        <p class="provider-setup-form__error" role="alert">
          {error}
        </p>
      )}
      {editing && auth?.message && (
        <p class="provider-setup-form__error" role="alert">
          Could not load GitHub repositories: {auth.message}. You can enter a repository path manually.
        </p>
      )}
    </form>
  );
}
