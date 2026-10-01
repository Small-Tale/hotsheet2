import './ticket-source-setup-dialog.css';
import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';

import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { ChevronLeft, ChevronRight, GitBranch, Power, PowerOff, Trash2 } from 'lucide';

import type { ProviderConnection } from '../api';
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
  { value: 'authorized', label: 'GitHub authorized' },
  { value: 'editing', label: "Editing this project's source" },
  { value: 'editing-machine', label: 'Editing for every project' },
  { value: 'removing', label: 'Confirm removal' },
  { value: 'busy', label: 'Saving connection' },
  { value: 'remote', label: 'Back up repository' },
];

export interface TicketSourceSetupProject {
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
  /** The edited connection awaiting confirmation of its permanent removal (HS2-724S9N). */
  removingProviderId?: string;
  /**
   * Where an edit was opened (HS2-3SCH1K): `project` edits the connection and this project's default
   * choice; `machine` (App Settings → Connections) edits it for every project and offers Disable and
   * Remove. Defaults to `project`.
   */
  editScope?: 'project' | 'machine';
  /** Whether the edited connection is this project's default source (project edits only). */
  projectDefault?: boolean;
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
  editScope = 'project',
  projectDefault = false,
  previewScenario,
}: TicketSourceSetupDialogProps) {
  const editing = providerConnections.find((item) => item.id === editingProviderId),
    machineEdit = Boolean(editing) && editScope === 'machine',
    disclosure = <LucideIcon icon={ChevronRight} name="chevron-right" />,
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
      data-action="connect-ticket-store-remote"
    >
      <wa-button
        class="provider-setup-form__back"
        appearance="plain"
        type="button"
        data-action="back-ticket-store-remote"
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
        defaultChoice={!editing ? true : machineEdit ? undefined : projectDefault}
      />
      {editing && (
        <p class="ticket-source-setup__scope-hint" data-edit-scope={editScope}>
          {machineEdit
            ? 'Changes apply to every project that uses this connection.'
            : 'Connection details are shared by every project that uses them. Disable or remove the connection under App Settings → Connections.'}
        </p>
      )}
    </>
  ) : (
    <></>
  );
  const rootActions = (
      <wa-button appearance="plain" data-action="dismiss-ticket-source-setup">
        Cancel
      </wa-button>
    ),
    detailActions = created ? (
      <>
        <wa-button appearance="plain" type="button" data-action="dismiss-ticket-source-setup">
          Skip for now
        </wa-button>
        <wa-button appearance="accent" type="button" data-action="submit-ticket-store-remote" disabled={remoteBusy}>
          {remoteBusy ? 'Connecting…' : 'Connect & push'}
        </wa-button>
      </>
    ) : machineEdit && editing && removingProviderId === editing.id ? (
      // One wrapping group, so the morph replaces the edit actions instead of recycling the clicked
      // "Remove data source…" button into "Keep" while that same click is still dispatching.
      <div class="ticket-source-setup__removal" role="group" aria-label="Confirm removal">
        <p class="ticket-source-setup__removal-prompt" role="alert">
          <strong>Remove {editing.name ?? editing.id}?</strong> It is unlinked from every project, and a sign-in Hot
          Sheet saved for it is deleted. Tickets stay in {providerName(editing.provider as ExternalProviderKind)}.
        </p>
        <wa-button appearance="plain" type="button" data-action="cancel-provider-removal" disabled={providerBusy}>
          Keep
        </wa-button>
        <wa-button
          variant="danger"
          appearance="accent"
          type="button"
          data-action="confirm-provider-removal"
          disabled={providerBusy}
        >
          {providerBusy ? 'Removing…' : 'Remove'}
        </wa-button>
      </div>
    ) : (
      <>
        {machineEdit && editing && (
          <wa-button
            class="ticket-source-setup__remove"
            variant="danger"
            appearance="plain"
            type="button"
            data-action="request-provider-removal"
            disabled={providerBusy}
          >
            <LucideIcon slot="start" icon={Trash2} name="trash-2" />
            Remove data source…
          </wa-button>
        )}
        {machineEdit && editing && (
          <wa-button
            class="ticket-source-setup__toggle"
            appearance="plain"
            type="button"
            data-action="toggle-provider-disabled"
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
        <wa-button appearance="plain" type="button" data-action="dismiss-ticket-source-setup">
          Cancel
        </wa-button>
        <wa-button
          appearance="accent"
          type="button"
          data-action="submit-provider-setup"
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
        label="Ticket source setup actions"
        a={rootActions}
        b={detailActions}
      />
    </wa-dialog>
  );
}
