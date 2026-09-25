import './provider-setup-form.css';
import './flow-back-button.css';

import { Grid } from '@kerfjs/ui/grid';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ChevronLeft } from 'lucide';

import type { ProviderConnection } from '../api';

export type ExternalProviderKind = 'github' | 'gitlab' | 'jira';

export interface GithubAuthState {
  session: string;
  userCode: string;
  verificationUri: string;
  state: 'waiting' | 'authorized' | 'denied' | 'expired' | 'cancelled' | 'error';
  credential?: string;
  repositories?: string[];
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
    repositories = auth?.repositories ?? [];
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
      {kind === 'github' && !editing && (
        <section class="provider-setup-form__github-auth" aria-label="GitHub sign in">
          <button type="button" data-action="start-github-sign-in" disabled={auth?.state === 'waiting'}>
            {auth?.state === 'waiting' ? 'Waiting for GitHub…' : 'Sign in with GitHub'}
          </button>
          {auth?.state === 'waiting' && (
            <>
              <p>
                Enter <strong>{auth.userCode}</strong> at{' '}
                <a href={auth.verificationUri} target="_blank" rel="noopener">
                  Open GitHub device authorization
                </a>
                .
              </p>
              <button type="button" data-action="cancel-github-sign-in">
                Cancel sign in
              </button>
            </>
          )}
          {auth?.state === 'authorized' && (
            <p role="status">
              Signed in securely.{' '}
              {repositories.length ? 'Choose an installed repository below.' : 'Loading installed repositories…'}
            </p>
          )}
          {auth?.message && <p role="alert">{auth.message}</p>}
          {auth && ['denied', 'expired', 'cancelled', 'error'].includes(auth.state) && (
            <p role="alert">{auth.message ?? `GitHub sign in was ${auth.state}.`}</p>
          )}
        </section>
      )}
      <Grid className="provider-setup-form__grid" columns={2} gap="m">
        <label>
          Connection ID
          <input
            name="connection-id"
            required
            placeholder={`${kind}-main`}
            value={connection?.id ?? ''}
            disabled={editing}
          />
        </label>
        <label>
          Display name
          <input name="connection-name" placeholder={labels[0]} value={connection?.name ?? ''} />
        </label>
        <label class="provider-setup-form__wide">
          {kind === 'jira' ? 'Project key' : 'Repository'}
          {kind === 'github' && !editing && repositories.length ? (
            <select name="connection-locator" required>
              <option value="">Choose a repository…</option>
              {repositories.map((repository) => (
                <option value={repository}>{repository}</option>
              ))}
            </select>
          ) : (
            <input name="connection-locator" required placeholder={labels[1]} value={connection?.locator ?? ''} />
          )}
        </label>
        {(kind !== 'github' || editing) && (
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
        {kind === 'github' && !editing && (
          <details class="provider-setup-form__wide">
            <summary>Use a credential reference instead</summary>
            <label>
              Credential reference
              <input name="credential-reference" placeholder={labels[2]} />
              <small>Advanced fallback for an existing OS-keychain credential.</small>
            </label>
          </details>
        )}
        {kind === 'jira' && (
          <>
            <label>
              Account email
              <input name="jira-email" type="email" required value={connectionSetting(connection, 'email')} />
            </label>
            <label>
              Jira site URL
              <input name="api-base" type="url" required placeholder="https://company.atlassian.net" value={apiBase} />
            </label>
          </>
        )}
        {kind !== 'jira' && (
          <label class="provider-setup-form__wide">
            API base URL (optional)
            <input
              name="api-base"
              type="url"
              placeholder={kind === 'github' ? 'https://api.github.com' : 'https://gitlab.com/api/v4'}
              value={apiBase}
            />
            <small>{kind === 'github' ? 'For GitHub Enterprise, enter its API URL before signing in.' : ''}</small>
          </label>
        )}
        <label class="provider-setup-form__option provider-setup-form__wide">
          <input name="make-default" type="checkbox" checked={connection?.default ?? true} /> Use as the default ticket
          source <span>New tickets will be created here.</span>
        </label>
      </Grid>
      {error && (
        <p class="provider-setup-form__error" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
