/** Ticket-source setup and settings surfaces over the demo connection fixtures (HS2-HMVBZW). */
import { signal } from 'kerfjs';

import { KeyboardSettings } from '../components/keyboard-settings';
import { ProviderSetupForm } from '../components/provider-setup-form';
import { SettingsWorkspace } from '../components/settings-workspace';
import { TicketSourceSetupDialog } from '../components/ticket-source-setup-dialog';
import { AccountsSettings, TicketSourcesSettings } from '../components/ticket-sources-settings';
import { TrashSettings } from '../components/trash-settings';
import type { DemoModule } from './demo-module';
import { DEMO_ACCOUNTS, DEMO_PROJECT_SOURCES } from './fixtures';
import {
  resetSettingsWorkspaceDemo,
  SettingsWorkspaceSettings,
  settingsWorkspaceSettings,
} from './settings-workspace-demo';

export type TicketSourceScenario =
  | 'root'
  | 'signed-out'
  | 'accounts'
  | 'jira-account'
  | 'waiting'
  | 'authorized'
  | 'authorized-existing-default'
  | 'editing'
  | 'editing-shared'
  | 'removing'
  | 'busy'
  | 'remote';
export const ticketSourceScenario = signal<TicketSourceScenario>('root');

export function KeyboardSettingsDemo() {
  return <KeyboardSettings overrides={{}} apple={true} />;
}

export function TicketSourceSetupDialogDemo() {
  const scenario = ticketSourceScenario.value,
    editing = ['editing', 'editing-shared', 'removing', 'busy'].includes(scenario),
    connection = {
      id: 'github-main',
      provider: 'github',
      locator: 'small-tale/hotsheet2',
      name: 'Product issues',
      default: true,
      settings: {
        attachment_repo: 'small-tale/hotsheet-assets',
        attachment_folder: 'hotsheet-attachments',
        attachment_branch: 'main',
      },
      projects:
        scenario === 'editing-shared'
          ? [
              { id: 'demo', alias: 'Demo project' },
              { id: 'marketing', alias: 'marketing-site' },
            ]
          : [{ id: 'demo', alias: 'Demo project' }],
    };
  return (
    <TicketSourceSetupDialog
      project={{ id: 'demo', root: '/work/demo', name: 'Demo project', stores: [], needsTicketSetup: true }}
      providerKind={
        scenario === 'root' || scenario === 'remote' ? undefined : scenario === 'jira-account' ? 'jira' : 'github'
      }
      providerConnections={editing ? [connection] : []}
      editingProviderId={editing ? connection.id : undefined}
      removingProviderId={scenario === 'removing' ? connection.id : undefined}
      accounts={scenario === 'accounts' || scenario === 'jira-account' ? DEMO_ACCOUNTS : []}
      chosenAccount={scenario === 'jira-account' ? 'jira-token' : undefined}
      projectDefault
      hasProjectDefault={scenario === 'authorized-existing-default'}
      providerBusy={scenario === 'busy'}
      githubAuth={
        scenario === 'waiting'
          ? {
              session: 'demo',
              userCode: 'ABCD-EFGH',
              verificationUri: 'https://github.com/login/device',
              state: 'waiting',
              copied: true,
            }
          : scenario === 'authorized' || scenario === 'authorized-existing-default' || editing
            ? {
                session: 'demo',
                userCode: 'ABCD-EFGH',
                verificationUri: 'https://github.com/login/device',
                state: 'authorized',
                repositories: ['small-tale/hotsheet2', 'small-tale/hotsheet-assets'],
                installations: [{ account: 'small-tale', selection: 'all' }],
              }
            : undefined
      }
      createdGitTicketStore={scenario === 'remote' ? '/work/demo.hs2' : undefined}
      previewScenario={scenario}
      navigation="none"
    />
  );
}

export function ProviderSetupFormDemo() {
  return (
    <ProviderSetupForm
      kind="github"
      defaultChoice
      auth={{
        session: 'demo',
        userCode: 'ABCD-EFGH',
        verificationUri: 'https://github.com/login/device',
        state: 'authorized',
        repositories: ['small-tale/hotsheet2'],
      }}
    />
  );
}

export function TicketSourcesSettingsDemo() {
  return <TicketSourcesSettings sources={DEMO_PROJECT_SOURCES} />;
}

export function AccountsSettingsDemo() {
  return (
    <>
      <AccountsSettings accounts={DEMO_ACCOUNTS} signingOut={undefined} />
      <AccountsSettings accounts={[]} error="Could not reach the Hot Sheet server." />
    </>
  );
}

export function SettingsWorkspaceDemo() {
  return (
    <SettingsWorkspace
      category={settingsWorkspaceSettings.category.value}
      sources={{ sources: DEMO_PROJECT_SOURCES.slice(0, 1) }}
      accounts={{ accounts: DEMO_ACCOUNTS }}
      ai={{ tools: [], selection: { tool: 'codex' }, loading: false, message: '' }}
      commands={{ commands: [] }}
      lifecycle={{ days: 30, message: '' }}
      terminals={{ inheritGlobalShellHistory: false, message: '' }}
      permissions={{
        automation: { action: settingsWorkspaceSettings.permissionAction.value, delayMs: 60_000 },
        delays: [0, 15_000, 60_000, 120_000],
      }}
      columns={{ hideVerified: false }}
      general={{ showLoadingActivity: true }}
      keyboard={{ overrides: {}, apple: true }}
    />
  );
}

export function TrashSettingsDemo() {
  return <TrashSettings days={14} />;
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'keyboard-settings': { Demo: KeyboardSettingsDemo },
    'ticket-source-setup-dialog': { Demo: TicketSourceSetupDialogDemo },
    'provider-setup-form': { Demo: ProviderSetupFormDemo },
    'ticket-sources-settings': { Demo: TicketSourcesSettingsDemo },
    'accounts-settings': { Demo: AccountsSettingsDemo },
    'settings-workspace': {
      Demo: SettingsWorkspaceDemo,
      Settings: SettingsWorkspaceSettings,
      reset: resetSettingsWorkspaceDemo,
    },
    'trash-settings': { Demo: TrashSettingsDemo },
  },
};
