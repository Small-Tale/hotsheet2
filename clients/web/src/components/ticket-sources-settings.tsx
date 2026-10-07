import './ticket-sources-settings.css';

import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select } from '@kerfjs/ui/select';
import { Cable, Database, LogOut, Unlink } from 'lucide';

import type { ProviderAccount } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { ProviderIcon, type ProviderIconKind } from './provider-icon';
import { type ExternalProviderKind, providerName } from './provider-setup-form';

/** One ticket source this project's checkout owns (HS2-3SCH1K, HS2-SM9PM8). */
export interface ProjectTicketSource {
  connectionId: string;
  name: string;
  provider: string;
  locator: string;
  /** This checkout's default source. */
  default: boolean;
  disabled?: boolean;
  /** Other projects that share this source (attached headlessly with `checkout add-source`). */
  sharedWith?: readonly string[];
}

export interface TicketSourcesSettingsProps {
  sources: readonly ProjectTicketSource[];
  error?: string;
  setupOpen?: boolean;
}

function sourceKind(provider: string) {
  return provider === 'git' ? 'Hot Sheet git' : providerName(provider as ExternalProviderKind);
}

function ConnectionCopy({
  name,
  provider,
  locator,
  isDefault = false,
  disabled = false,
  sharedWith = [],
}: {
  name: string;
  provider: string;
  locator: string;
  isDefault?: boolean;
  disabled?: boolean;
  sharedWith?: readonly string[];
}) {
  return (
    <span class="ticket-provider-settings__connection-copy">
      <strong>
        {name}
        {isDefault && <small>Default</small>}
        {disabled && <small data-state="disabled">Disabled</small>}
      </strong>
      <small>
        {sourceKind(provider)} · {locator}
      </small>
      {sharedWith.length > 0 && <small>Also used by {sharedWith.join(', ')}</small>}
    </span>
  );
}

/**
 * Project Settings → Ticket sources (HS2-3SCH1K, HS2-SM9PM8): only the sources this project owns —
 * never another project's. The default-source choice, editing, Disable, and Remove act on this
 * project's sources; sign-ins are machine-wide and live under App Settings → Accounts.
 */
export function TicketSourcesSettings({ sources, error = '', setupOpen = false }: TicketSourcesSettingsProps) {
  const defaultSource = sources.find((source) => source.default) ?? sources.at(0);
  return (
    <div class="ticket-provider-settings ticket-sources-settings" data-component="ticket-sources-settings">
      <section>
        <header class="ticket-provider-settings__header">
          <h2 class="ticket-provider-settings__title">Ticket sources</h2>
          <wa-button appearance="outlined" {...COMMANDS_AND_AI_ACTIONS.openProviderDialog.attrs}>
            Add data source
          </wa-button>
        </header>
        <p class="ticket-provider-settings__intro">
          This project uses {sources.length} ticket source{sources.length === 1 ? '' : 's'}. New tickets go to the
          default source unless you choose another when creating one.
        </p>
        {sources.length > 1 && (
          <div class="ticket-provider-settings__default">
            <Select
              name="project-default-source"
              label="Default source"
              value={defaultSource?.connectionId ?? ''}
              choices={sources.map((source) => ({
                value: source.connectionId,
                label: source.name,
                icon: source.provider === 'git' ? Database : Cable,
                iconName: source.provider === 'git' ? 'database' : 'cable',
              }))}
            />
          </div>
        )}
        {sources.length > 0 && (
          <div class="ticket-provider-settings__connections">
            <List>
              {sources.map((source, index) => {
                const copy = (
                  <ConnectionCopy
                    name={source.name}
                    provider={source.provider}
                    locator={source.locator}
                    isDefault={source.connectionId === defaultSource?.connectionId}
                    disabled={source.disabled}
                    sharedWith={source.sharedWith}
                  />
                );
                return source.provider === 'git' ? (
                  <div class="ticket-provider-settings__store" data-source-id={source.connectionId}>
                    <span class="ticket-provider-settings__store-icon">
                      <LucideIcon icon={Database} name="database" size="s" color={uiColor('neutral-on-quiet')} />
                    </span>
                    {copy}
                  </div>
                ) : (
                  <ListActionRow
                    action="edit-provider-connection"
                    itemId={source.connectionId}
                    multiline
                    divider={index > 0 ? 'before' : 'none'}
                    accessibleLabel={`Edit ${source.name}`}
                    icon={<LucideIcon icon={Cable} name="cable" />}
                    label={copy}
                    trailingAction="remove-project-source"
                    trailingActionLabel={`Remove ${source.name} from this project`}
                    trailingActionTitle="Remove from this project"
                    trailingActionIcon={<LucideIcon icon={Unlink} name="unlink" />}
                    trailingActionAttributes={{ 'data-source-id': source.connectionId }}
                  />
                );
              })}
            </List>
          </div>
        )}
      </section>
      {error && !setupOpen && (
        <p class="ticket-provider-settings__error" role="alert">
          {error}
        </p>
      )}
      <p class="ticket-provider-settings__footnote">
        Sources belong to this project; other projects never see them. GitHub, GitLab, and Jira sign-ins are shared by
        every project on this computer under{' '}
        <button
          type="button"
          class="ticket-provider-settings__link"
          {...COMMANDS_AND_AI_ACTIONS.selectSettingsCategory.attrs}
          data-item-id="accounts"
        >
          App Settings → Accounts
        </button>
        .
      </p>
    </div>
  );
}

export interface AccountsSettingsProps {
  accounts: readonly ProviderAccount[];
  error?: string;
  /** The account whose sign-out is in flight. */
  signingOut?: string;
  /** The orphaned connection selected for an inline removal confirmation. */
  sourceRemovalChoice?: string;
  removingSource?: string;
}

const accountProviderName = (provider: string) =>
  ({ github: 'GitHub', gitlab: 'GitLab', jira: 'Jira Cloud' })[provider] ?? provider;

function projectList(projects: readonly { alias: string }[]) {
  return projects.length ? `Used by ${projects.map((project) => project.alias).join(', ')}` : 'Not used by any project';
}

/**
 * App Settings → Accounts (HS2-SM9PM8): the machine-wide sign-ins ticket sources use, each with the
 * sources signed in through it and the projects that own them. Sources themselves belong to projects;
 * an account can be signed out only once no source uses it.
 */
export function AccountsSettings({
  accounts,
  error = '',
  signingOut,
  sourceRemovalChoice,
  removingSource,
}: AccountsSettingsProps) {
  return (
    <div class="ticket-provider-settings" data-component="accounts-settings">
      <section>
        <header class="ticket-provider-settings__header">
          <h2 class="ticket-provider-settings__title">Accounts</h2>
        </header>
        <p class="ticket-provider-settings__intro">
          Sign-ins Hot Sheet uses to reach GitHub, GitLab, and Jira. Every project on this computer can reuse them when
          adding a ticket source; each project still chooses its own repository or Jira project.
        </p>
        {accounts.length ? (
          <div class="ticket-provider-settings__accounts">
            {accounts.map((account) => {
              const name = accountProviderName(account.provider),
                host = account.host,
                kind = (
                  ['github', 'gitlab', 'jira'].includes(account.provider) ? account.provider : 'github'
                ) as ProviderIconKind,
                busy = signingOut === account.id;
              return (
                <article
                  class="ticket-provider-settings__connections ticket-provider-settings__account"
                  data-account-id={account.id}
                  aria-label={`${name} account ${account.identity ?? (account.host || account.id)}`}
                >
                  <header class="ticket-provider-settings__account-header">
                    <ProviderIcon kind={kind} size="l" />
                    <span class="ticket-provider-settings__connection-copy">
                      <strong>
                        {name}
                        {host && <span class="ticket-provider-settings__account-host">{host}</span>}
                      </strong>
                      <small>
                        {account.identity ? `${account.identity} · ` : ''}
                        {account.managed ? 'Signed in with Hot Sheet' : `Keychain credential ${account.id}`}
                      </small>
                    </span>
                    {(account.provider === 'github' && account.managed && !account.identity) ||
                    account.sources.length === 0 ? (
                      <span class="ticket-provider-settings__account-actions">
                        {account.provider === 'github' && account.managed && !account.identity && (
                          <wa-button
                            size="small"
                            appearance="outlined"
                            type="button"
                            {...COMMANDS_AND_AI_ACTIONS.identifyGithubAccount.attrs}
                            data-item-id={account.id}
                          >
                            Show username
                          </wa-button>
                        )}
                        {account.sources.length === 0 && (
                          <wa-button
                            size="small"
                            appearance="outlined"
                            type="button"
                            {...COMMANDS_AND_AI_ACTIONS.signOutAccount.attrs}
                            data-item-id={account.id}
                            disabled={busy}
                          >
                            <LucideIcon slot="start" icon={LogOut} name="log-out" />
                            {busy ? 'Signing out…' : 'Sign out'}
                          </wa-button>
                        )}
                      </span>
                    ) : null}
                  </header>
                  {account.sources.length ? (
                    account.sources.map((source) => {
                      const unused = source.projects.length === 0,
                        confirming = sourceRemovalChoice === source.connection_id,
                        removing = removingSource === source.connection_id;
                      return (
                        <div class="ticket-provider-settings__account-source" data-source-id={source.connection_id}>
                          <div class="ticket-provider-settings__store">
                            <span class="ticket-provider-settings__store-icon">
                              <LucideIcon icon={Cable} name="cable" size="s" color={uiColor('neutral-on-quiet')} />
                            </span>
                            <span class="ticket-provider-settings__connection-copy">
                              <strong>
                                {source.name}
                                {source.disabled && <small data-state="disabled">Disabled</small>}
                              </strong>
                              <small>
                                {source.locator} · {projectList(source.projects)}
                              </small>
                              {unused && <small>Connection ID: {source.connection_id}</small>}
                            </span>
                            {unused && !confirming && (
                              <wa-button
                                size="small"
                                appearance="outlined"
                                type="button"
                                {...COMMANDS_AND_AI_ACTIONS.requestUnusedAccountSourceRemoval.attrs}
                                data-account-id={account.id}
                                data-source-id={source.connection_id}
                              >
                                Remove
                              </wa-button>
                            )}
                          </div>
                          {unused && confirming && (
                            <div class="ticket-provider-settings__source-confirmation">
                              <span>Remove this unused connection? The sign-in stays available.</span>
                              <div>
                                <wa-button
                                  size="small"
                                  appearance="plain"
                                  type="button"
                                  {...COMMANDS_AND_AI_ACTIONS.cancelUnusedAccountSourceRemoval.attrs}
                                  disabled={removing}
                                >
                                  Keep
                                </wa-button>
                                <wa-button
                                  size="small"
                                  variant="danger"
                                  type="button"
                                  {...COMMANDS_AND_AI_ACTIONS.removeUnusedAccountSource.attrs}
                                  data-account-id={account.id}
                                  data-source-id={source.connection_id}
                                  disabled={removing}
                                >
                                  {removing ? 'Removing…' : 'Remove connection'}
                                </wa-button>
                              </div>
                            </div>
                          )}
                        </div>
                      );
                    })
                  ) : (
                    <p class="ticket-provider-settings__account-empty">
                      No ticket source uses this sign-in. Reuse it when adding a source to a project, or sign out.
                    </p>
                  )}
                </article>
              );
            })}
          </div>
        ) : (
          <p class="ticket-provider-settings__empty">
            No accounts yet. Signing in while adding a GitHub, GitLab, or Jira ticket source to a project adds one.
          </p>
        )}
      </section>
      {error && (
        <p class="ticket-provider-settings__error" role="alert">
          {error}
        </p>
      )}
      <p class="ticket-provider-settings__footnote">
        Credentials stay in the operating system keychain; Hot Sheet stores only their references and non-secret labels.
      </p>
    </div>
  );
}
