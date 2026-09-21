import './ticket-source-setup-dialog.css';

import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ChevronLeft, ChevronRight, GitBranch } from 'lucide';

import type { ProviderConnection } from '../api';
import { ContentTransition } from './content-transition';
import { ProviderIcon } from './provider-icon';
import {
  type ExternalProviderKind,
  type GithubAuthState,
  providerName,
  ProviderSetupForm,
} from './provider-setup-form';

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
}: TicketSourceSetupDialogProps) {
  const editing = providerConnections.find((item) => item.id === editingProviderId),
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
  ) => (
    <ListItem
      action={action}
      itemId={itemId}
      disabled={disabled}
      title={title}
      multiline
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
      <button class="provider-setup-form__back" type="button" data-action="back-ticket-store-remote">
        <LucideIcon icon={ChevronLeft} name="chevron-left" /> Ticket source types
      </button>
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
    <ProviderSetupForm kind={kind} connection={editing} auth={githubAuth} error={providerError} />
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
    ) : (
      <>
        <wa-button appearance="plain" type="button" data-action="dismiss-ticket-source-setup">
          Cancel
        </wa-button>
        <wa-button appearance="accent" type="button" data-action="submit-provider-setup" disabled={providerBusy}>
          {providerBusy ? 'Saving…' : editing ? 'Save changes' : 'Connect provider'}
        </wa-button>
      </>
    );
  return (
    <wa-dialog
      data-component="ticket-source-setup-dialog"
      data-ticket-source-setup-dialog
      data-navigation={navigation}
      label={detailLabel}
      with-footer
      open={Boolean(target)}
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
