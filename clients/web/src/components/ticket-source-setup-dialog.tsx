import './ticket-source-setup-dialog.css';
import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';

import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { ChevronLeft, ChevronRight, GitBranch, Power, PowerOff, Trash2 } from 'lucide';

import type { ProviderAccount, ProviderConnection } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ContentTransition } from './content-transition';
import { ProviderIcon } from './provider-icon';
import {
  type ExternalProviderKind,
  type GithubAuthState,
  providerName,
  ProviderSetupForm,
} from './provider-setup-form';

/** The UX demo's preview-state picker choices (production callers pass no `previewScenario`). */
const previewScenarioChoices: readonly SelectChoice[] = [
  { value: 'root', label: 'Choose source' },
  { value: 'signed-out', label: 'GitHub signed out' },
  { value: 'waiting', label: 'Waiting for GitHub' },
  { value: 'accounts', label: 'Reuse a signed-in account' },
  { value: 'authorized', label: 'GitHub authorized' },
  { value: 'editing', label: "Editing this project's source" },
  { value: 'editing-shared', label: 'Editing a shared source' },
  { value: 'removing', label: 'Confirm removal' },
  { value: 'busy', label: 'Saving connection' },
  { value: 'remote', label: 'Back up repository' },
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
  /** Machine-wide sign-ins a new source can reuse (HS2-SM9PM8). */
  accounts?: readonly ProviderAccount[];
  /** Demo-only state picker, rendered within the modal so its controls remain reachable. */
  previewScenario?: string;
}

export function TicketSourceSetupDialog({
  project: target,
  providerKind: kind,
  providerConnections,
  editingProviderId,
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
  accounts = [],
  previewScenario,
}: TicketSourceSetupDialogProps) {
  const editing = providerConnections.find((item) => item.id === editingProviderId),
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
      : kind
        ? `${editing ? 'Edit' : 'Connect'} ${providerName(kind)}`
        : rootLabel,
    active = created || kind ? 'b' : 'a',
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
          <small>{description}</small>
        </span>
      }
    />
  );
  const root = (
    <div class="ticket-source-setup ticket-source-setup__screen">
      <p class="ticket-source-setup__intro">
        {target?.needsTicketSetup ? (
          <>
            <strong>{target.name}</strong> is open, but it does not have a ticket source yet.
          </>
        ) : (
          <>
            Choose a ticket source to add to <strong>{target?.name}</strong>.
          </>
        )}
      </p>
      <nav class="ticket-source-setup__options" aria-label="Ticket source options">
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
      {setupError && (
        <p class="ticket-source-setup__error" role="alert">
          {setupError}
        </p>
      )}
    </div>
  );
  const remote = (
    <form
      id="ticket-source-remote-form"
      class="ticket-source-setup ticket-source-setup__screen ticket-source-setup__complete ticket-source-setup__remote-form"
      {...PROJECT_LIFECYCLE_ACTIONS.connectTicketStoreRemote.attrs}
    >
      <wa-button
        class="provider-setup-form__back"
        appearance="plain"
        type="button"
        {...PROJECT_LIFECYCLE_ACTIONS.backTicketStoreRemote.attrs}
      >
        <LucideIcon slot="start" icon={ChevronLeft} name="chevron-left" /> Ticket source types
      </wa-button>
      <div class="ticket-source-setup__remote">
        <LucideIcon icon={GitBranch} name="git-branch" />
        <div>
          <strong>Back up this ticket repository</strong>
          <p>Paste the clone URL from your Git host. Hot Sheet will connect this repository and make its first push.</p>
          <wa-input
            name="ticket-store-remote"
            type="text"
            label="Remote URL"
            placeholder="git@github.com:you/tickets.git"
            required
            autofocus
          ></wa-input>
          <a
            class="ticket-source-setup__remote-help"
            href="https://github.com/Small-Tale/hotsheet2/blob/main/docs/ticket-repository-remotes.md"
            target="_blank"
            rel="noopener"
          >
            How to create a remote repository
          </a>
        </div>
      </div>
      {remoteBusy && (
        <div class="ticket-source-setup__remote-progress" role="status">
          <wa-progress-bar indeterminate label="Connecting and pushing ticket repository"></wa-progress-bar>
          <p>Connecting the remote and uploading the ticket history. Large repositories can take several minutes.</p>
        </div>
      )}
      {remoteError && (
        <p class="ticket-source-setup__error" role="alert">
          {remoteError}
        </p>
      )}
    </form>
  );
  const detail = created ? (
    remote
  ) : kind ? (
    <>
      <ProviderSetupForm
        kind={kind}
        connection={editing}
        auth={githubAuth}
        error={providerError}
        defaultChoice={editing ? projectDefault : true}
        accounts={accounts}
      />
      {editing && sharedWith.length > 0 && (
        <p class="ticket-source-setup__scope-hint" data-shared-with={sharedWith.join(',')}>
          Also used by {sharedWith.join(', ')}. Changes to its details and Disable apply there too; removing it here
          leaves it in {sharedWith.length === 1 ? 'that project' : 'those projects'}.
        </p>
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
          Skip for now
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
    ) : editing && removingProviderId === editing.id ? (
      // One wrapping group, so the morph replaces the edit actions instead of recycling the clicked
      // "Remove from this project…" button into "Keep" while that same click is still dispatching.
      <div class="ticket-source-setup__removal" role="group" aria-label="Confirm removal">
        <p class="ticket-source-setup__removal-prompt" role="alert">
          <strong>
            Remove {editing.name ?? editing.id} from {target?.name ?? 'this project'}?
          </strong>{' '}
          {sharedWith.length
            ? `It stays in ${sharedWith.join(', ')}.`
            : 'No other project uses it, so its connection is deleted.'}{' '}
          Tickets stay in {providerName(editing.provider as ExternalProviderKind)}, and your sign-in stays under App
          Settings → Accounts.
        </p>
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
            Remove from this project…
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
