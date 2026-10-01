import './provider-setup-form.css';
import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';

import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { SunkenPanel } from '@kerfjs/ui/sunken-panel';
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
  /**
   * The "this project's default ticket source" checkbox state, or `undefined` to omit it, as when
   * editing a connection for every project from App Settings → Connections (HS2-3SCH1K).
   */
  defaultChoice?: boolean;
}

export function ProviderSetupForm({ kind, connection, auth, error = '', defaultChoice }: ProviderSetupFormProps) {
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
      <wa-button class="provider-setup-form__back" appearance="plain" type="button" data-action="back-provider-kind">
        <LucideIcon slot="start" icon={ChevronLeft} name="chevron-left" /> Ticket source types
      </wa-button>
      {/* Buttons carry `data-key` so a re-render never recycles the clicked control into a different
          action while that click is still dispatching (for example Enterprise ↔ GitHub.com; KF-HK7WE8). */}
      {kind === 'github' && !editing && (
        <SunkenPanel className="provider-setup-form__github-auth" ariaLabel="GitHub sign in">
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
                <wa-button appearance="accent" type="button" data-action="copy-github-code" data-key="copy-github-code">
                  <LucideIcon slot="start" icon={Copy} name="copy" /> {auth.copied ? 'Copy again' : 'Copy code'}
                </wa-button>
                <wa-button
                  appearance="plain"
                  type="button"
                  data-action="reopen-github-sign-in"
                  data-key="reopen-github-sign-in"
                >
                  <LucideIcon slot="start" icon={ExternalLink} name="external-link" /> Reopen GitHub
                </wa-button>
                <wa-button
                  appearance="plain"
                  type="button"
                  data-action="cancel-github-sign-in"
                  data-key="cancel-github-sign-in"
                >
                  Cancel
                </wa-button>
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
                  data-action="start-github-sign-in"
                  data-key="start-github-sign-in"
                >
                  <LucideIcon slot="start" icon={LogIn} name="log-in" /> Sign in with GitHub Enterprise
                </wa-button>
                <wa-button
                  appearance="plain"
                  type="button"
                  data-action="choose-github-dotcom"
                  data-key="choose-github-dotcom"
                >
                  Use GitHub.com instead
                </wa-button>
              </div>
            </>
          ) : (
            <>
              <p>
                Sign in to choose a repository. Hot Sheet opens GitHub in a small window and copies your one-time code
                for you.
              </p>
              <div class="provider-setup-form__auth-actions">
                <wa-button
                  appearance="accent"
                  type="button"
                  data-action="start-github-sign-in"
                  data-key="start-github-sign-in"
                >
                  <LucideIcon slot="start" icon={LogIn} name="log-in" /> Sign in with GitHub
                </wa-button>
                <wa-button
                  appearance="plain"
                  type="button"
                  data-action="choose-github-enterprise"
                  data-key="choose-github-enterprise"
                >
                  Use GitHub Enterprise…
                </wa-button>
              </div>
            </>
          )}
          {auth && ['denied', 'expired', 'cancelled', 'error'].includes(auth.state) && (
            <p role="alert">{auth.message ?? `GitHub sign in was ${auth.state}. Try again.`}</p>
          )}
          {auth?.message && !['denied', 'expired', 'cancelled', 'error'].includes(auth.state) ? (
            <p role="alert">{auth.message}</p>
          ) : (
            <></>
          )}
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
            <label class="provider-setup-form__wide">
              Repository
              {/* Native datalist keeps browser search over every reachable repository (HS2-27T5WT). */}
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
            ></wa-input>
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
            <wa-input
              class="provider-setup-form__wide"
              name="credential-reference"
              label="Credential reference"
              required
              placeholder={labels[2]}
              value={connectionCredential(connection)}
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
                value={connectionSetting(connection, 'email')}
              ></wa-input>
              <wa-input
                name="api-base"
                type="url"
                label="Jira site URL"
                required
                placeholder="https://company.atlassian.net"
                value={apiBase}
              ></wa-input>
            </>
          )}
          {kind === 'gitlab' && (
            <wa-input
              class="provider-setup-form__wide"
              name="api-base"
              type="url"
              label="API base URL (optional)"
              placeholder="https://gitlab.com/api/v4"
              value={apiBase}
            ></wa-input>
          )}
          {defaultChoice !== undefined && (
            <wa-checkbox class="provider-setup-form__wide" name="make-default" value="on" checked={defaultChoice}>
              Use as this project's default ticket source
              <span slot="hint">New tickets in this project will be created here.</span>
            </wa-checkbox>
          )}
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
