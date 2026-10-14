import './ticket-source-setup-dialog.css';
import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';

import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { ChevronRight, GitBranch, Power, PowerOff, Trash2 } from 'lucide';

import type { ProviderAccount, ProviderConnection } from '../api';
import { hostActionsAvailableHere, hostOnlyDisabledReason } from '../host-only-actions';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ContentTransition } from './content-transition';
import { ProviderIcon } from './provider-icon';
import {
  type ExternalProviderKind,
  type GithubAuthState,
  providerName,
  ProviderSetupBackButton,
  ProviderSetupForm,
} from './provider-setup-form';
import { TicketSourceColorPicker } from './ticket-source-color-picker';
import type { ProjectTicketSource } from './ticket-sources-settings';

/** The UX demo's preview-state picker choices (production callers pass no `previewScenario`). */
const previewScenarioChoices: readonly SelectChoice[] = [
  { value: 'root', label: 'Choose Source' },
  { value: 'signed-out', label: 'GitHub Signed Out' },
  { value: 'waiting', label: 'Waiting for GitHub' },
  { value: 'accounts', label: 'Reuse a Signed-In Account' },
  { value: 'jira-account', label: 'Reuse a Jira Account' },
  { value: 'authorized', label: 'GitHub Authorized' },
  { value: 'authorized-existing-default', label: 'GitHub Authorized, Existing Default' },
  { value: 'editing', label: "Editing This Project's Source" },
  { value: 'editing-shared', label: 'Editing a Shared Source' },
  { value: 'removing', label: 'Confirm Removal' },
  { value: 'busy', label: 'Saving Connection' },
  { value: 'remote', label: 'Back Up Repository' },
];

export interface TicketSourceSetupProject {
  /** The project's checkout id, to tell this project apart from others sharing a source. */
  id?: string;
  root: string;
  name: string;
  stores: readonly string[];
  needsTicketSetup?: boolean;
}

export interface TicketSourceSetupDialogProps {
  project?: TicketSourceSetupProject;
  providerKind?: ExternalProviderKind;
  providerConnections: readonly ProviderConnection[];
  editingProviderId?: string;
  editingGitSource?: ProjectTicketSource;
  githubAuth?: GithubAuthState;
  navigation: 'none' | 'push' | 'pop';
  createdGitTicketStore?: string;
  setupError?: string;
  remoteError?: string;
  remoteBusy?: boolean;
  providerBusy?: boolean;
  providerError?: string;
  /** The edited source awaiting confirmation of its removal from this project (HS2-724S9N, HS2-SM9PM8). */
  removingProviderId?: string;
  /** Whether the edited connection is this project's default source. */
  projectDefault?: boolean;
  /** Project-local icon color of the connection being edited. */
  sourceColor?: string;
  /** Whether this project already has any default ticket source. */
  hasProjectDefault?: boolean;
  /** Machine-wide sign-ins a new source can reuse (HS2-SM9PM8). */
  accounts?: readonly ProviderAccount[];
  /** The GitLab or Jira account whose details prefill a new source (HS2-F5HNJN). */
  chosenAccount?: string;
  /** Demo-only state picker, rendered within the modal so its controls remain reachable. */
  previewScenario?: string;
  /** False on a remote client: host-only controls render disabled with an explanation (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
}

export function TicketSourceSetupDialog({
  project: target,
  providerKind: kind,
  providerConnections,
  editingProviderId,
  editingGitSource,
  githubAuth,
  navigation,
  createdGitTicketStore = '',
  setupError = '',
  remoteError = '',
  remoteBusy = false,
  providerBusy = false,
  providerError = '',
  removingProviderId,
  projectDefault = false,
  sourceColor,
  hasProjectDefault = false,
  accounts = [],
  chosenAccount,
  previewScenario,
  hostActionsAvailable = hostActionsAvailableHere(),
}: TicketSourceSetupDialogProps) {
  const editing = providerConnections.find((item) => item.id === editingProviderId),
    editingGit = editingGitSource?.connectionId === editingProviderId ? editingGitSource : undefined,
    // Other projects that own the edited source too (attached headlessly; HS2-SM9PM8).
    sharedWith = (editing?.projects ?? [])
      .filter((project) => project.id !== target?.id)
      .map((project) => project.alias),
    disclosure = <LucideIcon icon={ChevronRight} name="chevron-right" size={16} />,
    created = createdGitTicketStore,
    defaultStore = `${target?.root}.hs2`,
    defaultExists = Boolean(target?.stores.includes(defaultStore));
  const rootLabel = 'Set up ticket support',
    detailLabel = created
      ? 'Ticket repository ready'
      : editingGit
        ? `Edit ${editingGit.name}`
        : kind
          ? `${editing ? 'Edit' : 'Connect'} ${providerName(kind)}`
          : rootLabel,
    active = created || kind || editingGit ? 'b' : 'a',
    navigationStyle = navigation === 'none' ? 'none' : 'push',
    direction = navigation === 'pop' ? 'backward' : 'forward';
  const option = (
    action: string,
    label: string,
    description: unknown,
    icon: unknown,
    itemId?: string,
    disabled = false,
    title?: string,
    divider: 'none' | 'before' = 'before',
  ) => (
    <ListItem
      action={action}
      itemId={itemId}
      disabled={disabled}
      title={title}
      multiline
      multilineIconAlign="center"
      divider={divider}
      accessibleLabel={label}
      icon={icon as never}
      trailing={disclosure}
      label={
        <span class="ticket-source-setup__option-copy">
          <strong>{label}</strong>
          <Text variant="span" tone="quiet" size="compact" wrap="anywhere">
            {description as never}
          </Text>
        </span>
      }
    />
  );
  const root = (
    <List gap="m">
      <Text tone="quiet" flush>
        {target?.needsTicketSetup ? (
          <>
            <strong>{target.name}</strong> is open, but it does not have a ticket source yet.
          </>
        ) : (
          <>
            Choose a ticket source to add to <strong>{target?.name}</strong>.
          </>
        )}
      </Text>
      <nav class="ticket-source-setup__options" aria-label="Ticket Source Options">
        {option(
          'create-project-git-source',
          'Create a Hot Sheet 2 git ticket repository',
          defaultExists ? (
            'Already connected.'
          ) : (
            <>
              Create and link <code>{defaultStore}</code>.
            </>
          ),
          <LucideIcon icon={GitBranch} name="git-branch" />,
          undefined,
          defaultExists,
          defaultExists ? 'The recommended ticket repository is already connected.' : undefined,
          'none',
        )}
        {option(
          'create-project-git-source-custom',
          'Create a Hot Sheet 2 git ticket repository in a custom location',
          'Choose another folder; multiple git ticket repositories are supported.',
          <LucideIcon icon={GitBranch} name="git-branch" />,
          undefined,
          !hostActionsAvailable,
          hostOnlyDisabledReason('choose-folder', hostActionsAvailable),
        )}
        {option(
          'select-provider-kind',
          'Connect GitHub Issues',
          'Connect an owner/repository.',
          <ProviderIcon kind="github" />,
          'github',
        )}
        {option(
          'select-provider-kind',
          'Connect GitLab Issues',
          'Connect a namespace/project.',
          <ProviderIcon kind="gitlab" />,
          'gitlab',
        )}
        {option(
          'select-provider-kind',
          'Connect Jira Cloud',
          'Connect a Jira project.',
          <ProviderIcon kind="jira" />,
          'jira',
        )}
      </nav>
      {setupError ? (
        <Text tone="danger" role="alert" wrap="anywhere" flush>
          {setupError}
        </Text>
      ) : (
        <></>
      )}
    </List>
  );
  const remote = (
    <form id="ticket-source-remote-form" {...PROJECT_LIFECYCLE_ACTIONS.connectTicketStoreRemote.attrs}>
      <List gap="m">
        <ProviderSetupBackButton action={PROJECT_LIFECYCLE_ACTIONS.backTicketStoreRemote} />
        <div class="ticket-source-setup__remote">
          <Row gap="xs" vAlign="top">
            <LucideIcon icon={GitBranch} name="git-branch" size="s" />
            <List gap="xs" flex>
              <strong>Back up this ticket repository</strong>
              <Text tone="quiet" flush>
                Paste the clone URL from your Git host. Hot Sheet will connect this repository and make its first push.
              </Text>
              <wa-input
                name="ticket-store-remote"
                type="text"
                label="Remote URL"
                placeholder="git@github.com:you/tickets.git"
                required
                autofocus
              />
              <Text variant="span" size="compact">
                <a
                  href="https://github.com/Small-Tale/hotsheet2/blob/main/docs/ticket-repository-remotes.md"
                  target="_blank"
                  rel="noopener"
                >
                  How to create a remote repository
                </a>
              </Text>
            </List>
          </Row>
        </div>
        {remoteBusy ? (
          <div role="status">
            <List gap="xs">
              <wa-progress-bar indeterminate label="Connecting and pushing ticket repository" />
              <Text size="compact" color={uiColor('brand-on-quiet')} flush>
                Connecting the remote and uploading the ticket history. Large repositories can take several minutes.
              </Text>
            </List>
          </div>
        ) : (
          <></>
        )}
        {remoteError ? (
          <Text tone="danger" role="alert" wrap="anywhere" flush>
            {remoteError}
          </Text>
        ) : (
          <></>
        )}
      </List>
    </form>
  );
  const detail = created ? (
    remote
  ) : editingGit ? (
    <div data-component="git-source-editor">
      <List gap="m">
        <Text tone="quiet" flush>
          {editingGit.default ? 'Default ticket source for this project.' : 'Ticket source for this project.'}
        </Text>
        <List gap="2xs">
          <Text variant="span" size="compact">
            <strong>Name</strong>
          </Text>
          <Text variant="span" wrap="anywhere">
            {editingGit.name}
          </Text>
        </List>
        <form id="git-source-relink-form" {...PROJECT_LIFECYCLE_ACTIONS.relinkGitSource.attrs}>
          <List gap="xs">
            <wa-input
              name="git-store-path"
              type="text"
              label="Location"
              value={editingGit.locator}
              required
              disabled={providerBusy}
            >
              <span slot="hint">If the ticket repository moved, enter its new folder path.</span>
            </wa-input>
            {editingGit.identityMismatch ? (
              <Text tone="danger" role="alert" wrap="anywhere" flush>
                This path contains a different Git store. Restore the original, or remove this source and add the
                replacement.
              </Text>
            ) : editingGit.storeUnavailable ? (
              <Text tone="danger" role="alert" wrap="anywhere" flush>
                This Git store cannot be opened. Restore its location, or remove this source.
              </Text>
            ) : (
              <wa-checkbox name="review-unverified-recovery" value="on">
                If the old repository identity was never recorded, I reviewed this folder and accept that Hot Sheet
                cannot verify it is the same repository.
              </wa-checkbox>
            )}
          </List>
        </form>
        {providerError ? (
          <Text tone="danger" role="alert" wrap="anywhere" flush>
            {providerError}
          </Text>
        ) : (
          <></>
        )}
        <List gap="xs">
          <TicketSourceColorPicker
            source={{ provider: 'git', name: editingGit.name, color: editingGit.color }}
            sourceId={editingGit.connectionId}
          />
          <Text variant="span" tone="quiet" size="compact">
            Color changes save when selected.
          </Text>
        </List>
      </List>
    </div>
  ) : kind ? (
    <>
      <ProviderSetupForm
        kind={kind}
        connection={editing}
        auth={githubAuth}
        error={providerError}
        defaultChoice={editing ? projectDefault : !hasProjectDefault}
        sourceColor={sourceColor}
        accounts={accounts}
        chosenAccount={chosenAccount}
      />
      {editing && sharedWith.length > 0 && (
        <Text
          class="ticket-source-setup__scope-hint"
          data-shared-with={sharedWith.join(',')}
          tone="quiet"
          size="compact"
          controlMargins="t"
          flush
        >
          Also used by {sharedWith.join(', ')}. Changes to its details and Disable apply there too; removing it here
          leaves it in {sharedWith.length === 1 ? 'that project' : 'those projects'}.
        </Text>
      )}
    </>
  ) : (
    <></>
  );
  const rootActions = (
      <wa-button
        class="ticket-source-setup__root-cancel"
        appearance="plain"
        {...PROJECT_LIFECYCLE_ACTIONS.dismissTicketSourceSetup.attrs}
      >
        Cancel
      </wa-button>
    ),
    detailActions = created ? (
      <>
        <wa-button appearance="plain" type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissTicketSourceSetup.attrs}>
          Skip for Now
        </wa-button>
        <wa-button
          appearance="accent"
          type="button"
          {...PROJECT_LIFECYCLE_ACTIONS.submitTicketStoreRemote.attrs}
          disabled={remoteBusy}
        >
          {remoteBusy ? 'Connecting…' : 'Connect & push'}
        </wa-button>
      </>
    ) : editingGit && removingProviderId === editingGit.connectionId ? (
      <div class="ticket-source-setup__removal" role="group" aria-label="Confirm Removal">
        <Text tone="quiet" role="alert" flush>
          <Text variant="span" tone="danger">
            <strong>
              Remove {editingGit.name} from {target?.name ?? 'this project'}?
            </strong>
          </Text>{' '}
          Tickets stay in the Git repository. You can add that repository again later.
        </Text>
        <Row hAlign="right" vAlign="middle" gap="xs">
          <wa-button appearance="plain" type="button" {...COMMANDS_AND_AI_ACTIONS.cancelProviderRemoval.attrs}>
            Keep
          </wa-button>
          <wa-button
            variant="danger"
            appearance="accent"
            type="button"
            {...COMMANDS_AND_AI_ACTIONS.confirmProviderRemoval.attrs}
            disabled={providerBusy}
          >
            {providerBusy ? 'Removing…' : 'Remove'}
          </wa-button>
        </Row>
      </div>
    ) : editingGit ? (
      <>
        <wa-button
          class="ticket-source-setup__remove"
          variant="danger"
          appearance="plain"
          type="button"
          {...COMMANDS_AND_AI_ACTIONS.requestProviderRemoval.attrs}
          disabled={providerBusy}
        >
          <LucideIcon slot="start" icon={Trash2} name="trash-2" />
          Remove from This Project…
        </wa-button>
        <wa-button appearance="plain" type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissTicketSourceSetup.attrs}>
          Done
        </wa-button>
        <wa-button
          appearance="accent"
          type="button"
          disabled={providerBusy}
          {...PROJECT_LIFECYCLE_ACTIONS.submitGitSourceRelink.attrs}
        >
          {providerBusy ? 'Saving…' : 'Save location'}
        </wa-button>
      </>
    ) : editing && removingProviderId === editing.id ? (
      // One wrapping group, so the morph replaces the edit actions instead of recycling the clicked
      // "Remove from this project…" button into "Keep" while that same click is still dispatching.
      <div class="ticket-source-setup__removal" role="group" aria-label="Confirm Removal">
        <Text tone="quiet" role="alert" flush>
          <Text variant="span" tone="danger">
            <strong>
              Remove {editing.name ?? editing.id} from {target?.name ?? 'this project'}?
            </strong>
          </Text>{' '}
          {sharedWith.length
            ? `It stays in ${sharedWith.join(', ')}.`
            : 'No other project uses it, so its connection is deleted.'}{' '}
          Tickets stay in {providerName(editing.provider as ExternalProviderKind)}, and your sign-in stays under App
          Settings → Accounts.
        </Text>
        <Row hAlign="right" vAlign="middle" gap="xs">
          <wa-button
            appearance="plain"
            type="button"
            {...COMMANDS_AND_AI_ACTIONS.cancelProviderRemoval.attrs}
            disabled={providerBusy}
          >
            Keep
          </wa-button>
          <wa-button
            variant="danger"
            appearance="accent"
            type="button"
            {...COMMANDS_AND_AI_ACTIONS.confirmProviderRemoval.attrs}
            disabled={providerBusy}
          >
            {providerBusy ? 'Removing…' : 'Remove'}
          </wa-button>
        </Row>
      </div>
    ) : (
      <>
        {editing && (
          <wa-button
            class="ticket-source-setup__remove"
            variant="danger"
            appearance="plain"
            type="button"
            {...COMMANDS_AND_AI_ACTIONS.requestProviderRemoval.attrs}
            disabled={providerBusy}
          >
            <LucideIcon slot="start" icon={Trash2} name="trash-2" />
            Remove from This Project…
          </wa-button>
        )}
        {editing && (
          <wa-button
            class="ticket-source-setup__toggle"
            appearance="plain"
            type="button"
            {...COMMANDS_AND_AI_ACTIONS.toggleProviderDisabled.attrs}
            disabled={providerBusy}
          >
            <LucideIcon
              slot="start"
              icon={editing.disabled ? Power : PowerOff}
              name={editing.disabled ? 'power' : 'power-off'}
            />
            {editing.disabled ? 'Enable' : 'Disable'}
          </wa-button>
        )}
        <wa-button appearance="plain" type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissTicketSourceSetup.attrs}>
          Cancel
        </wa-button>
        <wa-button
          appearance="accent"
          type="button"
          {...COMMANDS_AND_AI_ACTIONS.submitProviderSetup.attrs}
          disabled={providerBusy || (kind === 'github' && !editing && githubAuth?.state !== 'authorized')}
        >
          {providerBusy ? 'Saving…' : editing ? 'Save changes' : 'Connect provider'}
        </wa-button>
      </>
    );
  return (
    <wa-dialog
      data-key="ticket-source-setup-dialog"
      class="ticket-source-setup-dialog"
      data-component="ticket-source-setup-dialog"
      data-ticket-source-setup-dialog
      data-navigation={navigation}
      data-preview-scenario={previewScenario}
      label={detailLabel}
      with-footer
      open={Boolean(target)}
      data-controlled-open={String(Boolean(target))}
    >
      <ContentTransition
        active={active}
        style="crossfade"
        direction={direction}
        region="label"
        label="Ticket source setup title"
        a={rootLabel as never}
        b={detailLabel as never}
      />
      {previewScenario && (
        <div data-demo-ticket-source-scenario>
          <Select
            name="scenario"
            label="Preview dialog state"
            value={previewScenario}
            choices={previewScenarioChoices}
          />
        </div>
      )}
      <ContentTransition
        active={active}
        style={navigationStyle}
        direction={direction}
        label="Ticket source setup navigation"
        a={root}
        b={detail}
      />
      <ContentTransition
        active={active}
        style="crossfade"
        direction={direction}
        region="footer"
        sideLayout="actions"
        label="Ticket source setup actions"
        a={rootActions}
        b={detailActions}
      />
    </wa-dialog>
  );
}
