import './provider-setup-form.css';
import './flow-back-button.css';

import { Grid } from '@kerfjs/ui/grid';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ChevronLeft, Copy, ExternalLink, LogIn, RefreshCw } from 'lucide';

import type { ProviderConnection } from '../api';

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
}

export function ProviderSetupForm({ kind, connection, auth, error = '' }: ProviderSetupFormProps) {
  const labels = {
      github: ['GitHub Issues', 'owner/repository', 'GitHub credential reference'],
      gitlab: ['GitLab Issues', 'namespace/project', 'GitLab credential reference'],
      jira: ['Jira Cloud', 'PROJECT', 'Jira API-token reference'],
    }[kind],
    editing = Boolean(connection),
    apiBase = connectionSetting(connection, kind === 'jira' ? 'base_url' : 'api_base'),
    repositories = auth?.repositories ?? [],
    signedIn = auth?.state === 'authorized',
    // A new GitHub connection starts from sign-in; its settings appear once GitHub has authorized it.
    showFields = kind !== 'github' || editing || signedIn,
    choosing = kind === 'github' && !editing && signedIn && auth.repositories !== undefined,
    limited = (auth?.installations ?? []).filter((installation) => installation.selection !== 'all');
  return (
    <form
      id="provider-setup-form"
      class="provider-setup-form"
      data-component="provider-setup-form"
      data-action="save-provider-connection"
    >
      <button class="provider-setup-form__back" type="button" data-action="back-provider-kind">
        <LucideIcon icon={ChevronLeft} name="chevron-left" /> Ticket source types
      </button>
      {/* Buttons carry `data-key` so a re-render never recycles the clicked control into a different
          action while that click is still dispatching (for example Enterprise ↔ GitHub.com; KF-HK7WE8). */}
      {kind === 'github' && !editing && (
        <section
          class="provider-setup-form__github-auth"
          aria-label="GitHub sign in"
          data-state={auth?.state ?? 'idle'}
        >
          {auth?.state === 'waiting' ? (
            <>
              <p>
                Paste this one-time code in the GitHub window
                {auth.copied ? ' — it is already on your clipboard' : ''}, then approve Hot Sheet and close that window.
              </p>
              <output class="provider-setup-form__device-code" aria-label="One-time GitHub code">
                {auth.userCode}
              </output>
              <div class="provider-setup-form__auth-actions">
                <button type="button" data-action="copy-github-code" data-key="copy-github-code">
                  <LucideIcon icon={Copy} name="copy" /> {auth.copied ? 'Copy again' : 'Copy code'}
                </button>
                <button type="button" data-action="reopen-github-sign-in" data-key="reopen-github-sign-in">
                  <LucideIcon icon={ExternalLink} name="external-link" /> Reopen GitHub
                </button>
                <button type="button" data-action="cancel-github-sign-in" data-key="cancel-github-sign-in">
                  Cancel
                </button>
              </div>
              <p class="provider-setup-form__auth-waiting" role="status">
                Waiting for GitHub…
              </p>
            </>
          ) : signedIn ? (
            <p role="status">
              Signed in to {auth.enterpriseUrl ? new URL(auth.enterpriseUrl).host : 'GitHub'}.{' '}
              {auth.repositories ? 'Choose a repository below.' : 'Loading your repositories…'}
            </p>
          ) : auth?.enterprise ? (
            <>
              <label>
                GitHub Enterprise server URL
                <input
                  name="github-enterprise-url"
                  type="url"
                  required
                  placeholder="https://github.example.com"
                  value={auth.enterpriseUrl ?? ''}
                />
                <small>The address you use to open GitHub Enterprise in a browser.</small>
              </label>
              <div class="provider-setup-form__auth-actions">
                <button type="button" data-action="start-github-sign-in" data-key="start-github-sign-in">
                  <LucideIcon icon={LogIn} name="log-in" /> Sign in with GitHub Enterprise
                </button>
                <button type="button" data-action="choose-github-dotcom" data-key="choose-github-dotcom">
                  Use GitHub.com instead
                </button>
              </div>
            </>
          ) : (
            <>
              <p>
                Sign in to choose a repository. Hot Sheet opens GitHub in a small window and copies your one-time code
                for you.
              </p>
              <div class="provider-setup-form__auth-actions">
                <button type="button" data-action="start-github-sign-in" data-key="start-github-sign-in">
                  <LucideIcon icon={LogIn} name="log-in" /> Sign in with GitHub
                </button>
                <button type="button" data-action="choose-github-enterprise" data-key="choose-github-enterprise">
                  Use GitHub Enterprise…
                </button>
              </div>
            </>
          )}
          {auth && ['denied', 'expired', 'cancelled', 'error'].includes(auth.state) && (
            <p role="alert">{auth.message ?? `GitHub sign in was ${auth.state}. Try again.`}</p>
          )}
          {auth?.message && !['denied', 'expired', 'cancelled', 'error'].includes(auth.state) && (
            <p role="alert">{auth.message}</p>
          )}
        </section>
      )}
      {showFields && (
        <Grid className="provider-setup-form__grid" columns={2} gap="m">
          <label class="provider-setup-form__wide">
            Display name
            <input name="connection-name" placeholder={labels[0]} value={connection?.name ?? ''} />
            <small>Leave blank to use “{labels[0]}”.</small>
          </label>
          <label class="provider-setup-form__wide">
            {kind === 'jira' ? 'Project key' : 'Repository'}
            {choosing ? (
              // A searchable list: typing filters every repository the app can reach (HS2-27T5WT).
              <>
                <input
                  name="connection-locator"
                  required
                  list="provider-setup-github-repositories"
                  autocomplete="off"
                  placeholder="Search your repositories…"
                />
                <datalist id="provider-setup-github-repositories">
                  {repositories.map((repository) => (
                    <option value={repository}></option>
                  ))}
                </datalist>
                <small>
                  {repositories.length === 1 ? '1 repository' : `${repositories.length} repositories`} available. Type
                  to filter.
                </small>
              </>
            ) : (
              <input name="connection-locator" required placeholder={labels[1]} value={connection?.locator ?? ''} />
            )}
          </label>
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
                      Change access for {installation.account} <LucideIcon icon={ExternalLink} name="external-link" />
                    </a>
                  ))}
                {auth.installUrl && (
                  <a href={auth.installUrl} target="_blank" rel="noopener">
                    Add another account or organization <LucideIcon icon={ExternalLink} name="external-link" />
                  </a>
                )}
                <button
                  type="button"
                  data-action="refresh-github-repositories"
                  data-key="refresh-github-repositories"
                  disabled={auth.refreshing}
                >
                  <LucideIcon icon={RefreshCw} name="refresh-cw" /> {auth.refreshing ? 'Refreshing…' : 'Refresh list'}
                </button>
              </div>
            </section>
          )}
          {/* GitHub uses the credential saved by Sign in with GitHub; there is no manual reference (HS2-48GA17). */}
          {kind !== 'github' && (
            <label class="provider-setup-form__wide">
              Credential reference
              <input
                name="credential-reference"
                required
                placeholder={labels[2]}
                value={connectionCredential(connection)}
              />
              <small>
                Create this reference first with <code>hotsheet key set</code>; the token is never returned to the
                browser.
              </small>
            </label>
          )}
          {kind === 'jira' && (
            <>
              <label>
                Account email
                <input name="jira-email" type="email" required value={connectionSetting(connection, 'email')} />
              </label>
              <label>
                Jira site URL
                <input
                  name="api-base"
                  type="url"
                  required
                  placeholder="https://company.atlassian.net"
                  value={apiBase}
                />
              </label>
            </>
          )}
          {kind === 'gitlab' && (
            <label class="provider-setup-form__wide">
              API base URL (optional)
              <input name="api-base" type="url" placeholder="https://gitlab.com/api/v4" value={apiBase} />
            </label>
          )}
          <label class="provider-setup-form__option provider-setup-form__wide">
            <input name="make-default" type="checkbox" checked={connection?.default ?? true} /> Use as the default
            ticket source <span>New tickets will be created here.</span>
          </label>
        </Grid>
      )}
      {error && (
        <p class="provider-setup-form__error" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
