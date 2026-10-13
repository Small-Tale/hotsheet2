import './ticket-sources-settings.css';

import { Badge } from '@kerfjs/ui/badge';
import { rem, uiColor } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text, type TextContent } from '@kerfjs/ui/text';
import { Cable, ChevronRight, Database, LogOut, Unlink } from 'lucide';

import type { ProviderAccount } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { resolveTicketSourceColor } from './customization-palette';
import { ProviderIcon, type ProviderIconKind } from './provider-icon';
import { type ExternalProviderKind, providerName } from './provider-setup-form';
import { TicketSourceIcon } from './ticket-source-icon';

/** One ticket source this project's checkout owns (HS2-3SCH1K, HS2-SM9PM8). */
export interface ProjectTicketSource {
  connectionId: string;
  name: string;
  provider: string;
  locator: string;
  /** This checkout's default source. */
  default: boolean;
  color?: string;
  disabled?: boolean;
  unverifiedRecovery?: boolean;
  identityReviewRequired?: boolean;
  identityMismatch?: boolean;
  storeUnavailable?: boolean;
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

/** One quiet, wrapping secondary line of a source or account (HS2-8JY7J2). */
function Detail({ children }: { children: TextContent }) {
  return (
    <Text variant="span" tone="quiet" size="compact" wrap="anywhere">
      {children}
    </Text>
  );
}

function DisabledBadge() {
  return (
    <span data-state="disabled">
      <Badge tone="neutral" size="compact">
        Disabled
      </Badge>
    </span>
  );
}

function RecoveryBadge({ state, label }: { state: string; label: string }) {
  return (
    <span class="ticket-provider-settings__recovery-badge" data-state={state}>
      <Badge tone="info" size="compact">
        {label}
      </Badge>
    </span>
  );
}

function ConnectionCopy({
  name,
  provider,
  locator,
  isDefault = false,
  disabled = false,
  unverifiedRecovery = false,
  identityReviewRequired = false,
  identityMismatch = false,
  storeUnavailable = false,
  sharedWith = [],
}: {
  name: string;
  provider: string;
  locator: string;
  isDefault?: boolean;
  disabled?: boolean;
  unverifiedRecovery?: boolean;
  identityReviewRequired?: boolean;
  identityMismatch?: boolean;
  storeUnavailable?: boolean;
  sharedWith?: readonly string[];
}) {
  return (
    <span class="ticket-provider-settings__connection-copy">
      <strong>
        {name}
        {isDefault ? (
          <Badge tone="info" size="compact">
            Default
          </Badge>
        ) : (
          <></>
        )}
        {disabled ? <DisabledBadge /> : <></>}
      </strong>
      {unverifiedRecovery && <RecoveryBadge state="unverified-recovery" label="Unverified recovery" />}
      {identityReviewRequired && <RecoveryBadge state="identity-review-required" label="Review required" />}
      {identityMismatch && <RecoveryBadge state="identity-mismatch" label="Identity mismatch" />}
      {storeUnavailable && <RecoveryBadge state="store-unavailable" label="Store unavailable" />}
      <Detail>
        {sourceKind(provider)} · {locator}
      </Detail>
      {sharedWith.length > 0 ? <Detail>Also used by {sharedWith.join(', ')}</Detail> : <></>}
      {unverifiedRecovery ? <Detail>The former repository identity could not be verified.</Detail> : <></>}
      {identityReviewRequired ? <Detail>Review this source location before opening its tickets.</Detail> : <></>}
      {identityMismatch ? (
        <Detail>
          This path contains a different Git store. Restore the original, or remove and add this source again.
        </Detail>
      ) : (
        <></>
      )}
      {storeUnavailable ? (
        <Detail>This Git store cannot be opened. Restore its location, or remove this source.</Detail>
      ) : (
        <></>
      )}
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
  const duplicates = sources.filter(
    (source, index) =>
      sources.findIndex(
        (candidate) =>
          candidate.provider === source.provider &&
          resolveTicketSourceColor(candidate.color) === resolveTicketSourceColor(source.color),
      ) < index,
  );
  return (
    <div class="ticket-provider-settings ticket-sources-settings" data-component="ticket-sources-settings">
      <List gap="l">
        <List gap="m">
          {/* The workspace toolbar titles the page (HS2-FV538C) and carries its "+ Data Source…"
              action (HS2-G9J35B). */}
          <Text tone="quiet" flush>
            This project uses {sources.length} ticket source{sources.length === 1 ? '' : 's'}. New tickets go to the
            default source unless you choose another when creating one.
          </Text>
          {duplicates.length > 0 ? (
            <StateBanner
              title="Some ticket sources look alike"
              detail="Choose a different color for sources from the same provider to tell their tickets apart."
              tone="warning"
              urgency="status"
              copyLayout="stacked"
              icon={<LucideIcon icon={Cable} name="cable" />}
            />
          ) : (
            <></>
          )}
          {sources.length > 1 ? (
            <Grid minColumnWidth={rem(22)} autoFill>
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
            </Grid>
          ) : (
            <></>
          )}
          {sources.length > 0 ? (
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
                      unverifiedRecovery={source.unverifiedRecovery}
                      identityReviewRequired={source.identityReviewRequired}
                      identityMismatch={source.identityMismatch}
                      storeUnavailable={source.storeUnavailable}
                      sharedWith={source.sharedWith}
                    />
                  );
                  const row = {
                    action: 'edit-provider-connection',
                    itemId: source.connectionId,
                    multiline: true as const,
                    divider: index > 0 ? ('before' as const) : ('none' as const),
                    accessibleLabel: `Edit ${source.name}`,
                    icon: (
                      <TicketSourceIcon
                        source={{ provider: source.provider, name: source.name, color: source.color }}
                        size="compact"
                      />
                    ),
                    label: copy,
                    // The source hooks ride on the Kerf row root itself, so no app wrapper (or CSS)
                    // has to size that root (HS2-Y49C09).
                    rootAttributes: {
                      'data-source-row': '',
                      'data-source-id': source.connectionId,
                      'data-source-provider': source.provider,
                    },
                  };
                  return source.provider === 'git' ? (
                    <ListItem {...row} trailing={<LucideIcon icon={ChevronRight} name="chevron-right" size={16} />} />
                  ) : (
                    <ListActionRow
                      {...row}
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
          ) : (
            <></>
          )}
        </List>
        {error && !setupOpen ? (
          <Text tone="danger" role="alert" flush>
            {error}
          </Text>
        ) : (
          <></>
        )}
        <Text tone="quiet" class="ticket-provider-settings__footnote" flush>
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
        </Text>
      </List>
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
      <List gap="l">
        <List gap="m">
          {/* The workspace toolbar already titles the page (HS2-MF9D1K). */}
          <Text tone="quiet" flush>
            Sign-ins Hot Sheet uses to reach GitHub, GitLab, and Jira. Every project on this computer can reuse them
            when adding a ticket source; each project still chooses its own repository or Jira project.
          </Text>
          {accounts.length ? (
            <List gap="m">
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
                        <Detail>
                          {account.identity ? `${account.identity} · ` : ''}
                          {account.managed ? 'Signed in with Hot Sheet' : `Keychain credential ${account.id}`}
                        </Detail>
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
                            <Row vAlign="middle" gap="xs" wrap>
                              <LucideIcon icon={Cable} name="cable" size="s" color={uiColor('neutral-on-quiet')} />
                              <span class="ticket-provider-settings__connection-copy">
                                <strong>
                                  {source.name}
                                  {source.disabled ? <DisabledBadge /> : <></>}
                                </strong>
                                <Detail>
                                  {source.locator} · {projectList(source.projects)}
                                </Detail>
                                {unused ? <Detail>Connection ID: {source.connection_id}</Detail> : <></>}
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
                            </Row>
                            {unused && confirming && (
                              <Row hAlign="space-between" vAlign="middle" gap="xs" wrap>
                                <Text variant="span" tone="quiet" size="compact">
                                  Remove this unused connection? The sign-in stays available.
                                </Text>
                                <Row gap="xs">
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
                                </Row>
                              </Row>
                            )}
                          </div>
                        );
                      })
                    ) : (
                      <div class="ticket-provider-settings__account-empty">
                        <Text tone="quiet" size="compact" flush>
                          No ticket source uses this sign-in. Reuse it when adding a source to a project, or sign out.
                        </Text>
                      </div>
                    )}
                  </article>
                );
              })}
            </List>
          ) : (
            <Text tone="quiet" flush>
              No accounts yet. Signing in while adding a GitHub, GitLab, or Jira ticket source to a project adds one.
            </Text>
          )}
        </List>
        {error ? (
          <Text tone="danger" role="alert" flush>
            {error}
          </Text>
        ) : (
          <></>
        )}
        <Text tone="quiet" class="ticket-provider-settings__footnote" flush>
          Credentials stay in the operating system keychain; Hot Sheet stores only their references and non-secret
          labels.
        </Text>
      </List>
    </div>
  );
}
