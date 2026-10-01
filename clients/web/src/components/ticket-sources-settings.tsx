import './ticket-sources-settings.css';

import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select } from '@kerfjs/ui/select';
import { Cable, ChevronRight, Database, Plus, Unlink } from 'lucide';

import type { ProviderConnection } from '../api';
import { type ExternalProviderKind, providerName } from './provider-setup-form';

/** One ticket source this project's checkout links (HS2-3SCH1K). */
export interface ProjectTicketSource {
  connectionId: string;
  name: string;
  provider: string;
  locator: string;
  /** This checkout's default source, not the machine-wide registry's. */
  default: boolean;
  disabled?: boolean;
}

export interface TicketSourcesSettingsProps {
  sources: readonly ProjectTicketSource[];
  /** Machine-wide connections this project does not use yet. */
  available?: readonly ProviderConnection[];
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
}: {
  name: string;
  provider: string;
  locator: string;
  isDefault?: boolean;
  disabled?: boolean;
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
    </span>
  );
}

/**
 * Project Settings → Ticket sources (HS2-3SCH1K): only the sources this checkout links. The
 * default-source choice and Detach act on this checkout alone; editing a connection's details,
 * disabling it, or removing it for every project lives under App Settings → Connections.
 */
export function TicketSourcesSettings({
  sources,
  available = [],
  error = '',
  setupOpen = false,
}: TicketSourcesSettingsProps) {
  const defaultSource = sources.find((source) => source.default) ?? sources.at(0);
  return (
    <div class="ticket-provider-settings" data-component="ticket-sources-settings">
      <section>
        <header class="ticket-provider-settings__header">
          <h2>Ticket sources</h2>
          <wa-button appearance="outlined" data-action="open-provider-dialog">
            Add data source
          </wa-button>
        </header>
        <p>
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
                  />
                );
                return source.provider === 'git' ? (
                  <div class="ticket-provider-settings__store" data-source-id={source.connectionId}>
                    <LucideIcon icon={Database} name="database" />
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
                    trailingAction="detach-project-source"
                    trailingActionLabel={`Detach ${source.name} from this project`}
                    trailingActionTitle="Detach from this project"
                    trailingActionIcon={<LucideIcon icon={Unlink} name="unlink" />}
                    trailingActionAttributes={{ 'data-source-id': source.connectionId }}
                  />
                );
              })}
            </List>
          </div>
        )}
      </section>
      {available.length > 0 && (
        <section>
          <h2>Other connections on this machine</h2>
          <p>Connected for other projects. Use one here without signing in again.</p>
          <div class="ticket-provider-settings__connections">
            <List>
              {available.map((connection, index) => (
                <ListItem
                  action="attach-project-source"
                  itemId={connection.id}
                  multiline
                  divider={index > 0 ? 'before' : 'none'}
                  accessibleLabel={`Use ${connection.name ?? connection.id} in this project`}
                  icon={<LucideIcon icon={Cable} name="cable" />}
                  trailing={<LucideIcon icon={Plus} name="plus" />}
                  label={
                    <ConnectionCopy
                      name={connection.name ?? connection.id}
                      provider={connection.provider}
                      locator={connection.locator}
                      disabled={connection.disabled}
                    />
                  }
                />
              ))}
            </List>
          </div>
        </section>
      )}
      {error && !setupOpen && (
        <p class="ticket-provider-settings__error" role="alert">
          {error}
        </p>
      )}
      <p>
        To change sign-in details, disable, or remove a connection for every project, use{' '}
        <button
          type="button"
          class="ticket-provider-settings__link"
          data-action="select-settings-category"
          data-item-id="connections"
        >
          App Settings → Connections
        </button>
        .
      </p>
    </div>
  );
}

export interface ConnectionsSettingsProps {
  connections: readonly ProviderConnection[];
  error?: string;
  setupOpen?: boolean;
}

/**
 * App Settings → Connections (HS2-3SCH1K): the machine-wide ticket-provider connection catalog.
 * Edits, Disable, and Remove here affect every project that uses the connection.
 */
export function ConnectionsSettings({ connections, error = '', setupOpen = false }: ConnectionsSettingsProps) {
  return (
    <div class="ticket-provider-settings" data-component="connections-settings">
      <section>
        <header class="ticket-provider-settings__header">
          <h2>Connections</h2>
        </header>
        <p>
          Ticket-provider connections on this machine, shared by every project that uses them. Changing, disabling, or
          removing one affects all of those projects.
        </p>
        {connections.length ? (
          <div class="ticket-provider-settings__connections">
            <List>
              {connections.map((connection, index) => (
                <ListItem
                  action="edit-provider-connection"
                  itemId={connection.id}
                  rootAttributes={{ 'data-edit-scope': 'machine' }}
                  multiline
                  divider={index > 0 ? 'before' : 'none'}
                  accessibleLabel={`Edit ${connection.name ?? connection.id} for every project`}
                  icon={<LucideIcon icon={Cable} name="cable" />}
                  trailing={<LucideIcon icon={ChevronRight} name="chevron-right" />}
                  label={
                    <ConnectionCopy
                      name={connection.name ?? connection.id}
                      provider={connection.provider}
                      locator={connection.locator}
                      disabled={connection.disabled}
                    />
                  }
                />
              ))}
            </List>
          </div>
        ) : (
          <p class="ticket-provider-settings__empty">
            No external connections yet. Add one from a project's Ticket sources settings.
          </p>
        )}
      </section>
      {error && !setupOpen && (
        <p class="ticket-provider-settings__error" role="alert">
          {error}
        </p>
      )}
      <p>
        Connection metadata is stored in <code>providers.json</code>; credentials remain in the OS keychain.
      </p>
    </div>
  );
}
