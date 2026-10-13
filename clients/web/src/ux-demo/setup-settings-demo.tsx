import { CatalogExample } from '@kerfjs/ui/catalog';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { KeyboardSettings } from '../components/keyboard-settings';
import { ProviderSetupForm } from '../components/provider-setup-form';
import { type SettingsCategory } from '../components/settings-navigation';
import { SettingsWorkspace } from '../components/settings-workspace';
import { TicketSourceSetupDialog } from '../components/ticket-source-setup-dialog';
import { AccountsSettings, TicketSourcesSettings } from '../components/ticket-sources-settings';
import { TrashSettings } from '../components/trash-settings';
import { type PermissionAutomationAction } from '../permission-notifications';
import type { DemoModule, FormControl } from './demo-module';
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
  return (
    <CatalogExample label="Apple keyboard">
      <KeyboardSettings overrides={{}} apple={true} />
    </CatalogExample>
  );
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
    <CatalogExample label="Interactive">
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
    </CatalogExample>
  );
}

export function ProviderSetupFormDemo() {
  return (
    <CatalogExample label="GitHub, authorized">
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
    </CatalogExample>
  );
}

export function TicketSourcesSettingsDemo() {
  return (
    <CatalogExample label="Connected sources">
      <TicketSourcesSettings sources={DEMO_PROJECT_SOURCES} />
    </CatalogExample>
  );
}

export function AccountsSettingsDemo() {
  return (
    <>
      <CatalogExample label="Signed-in accounts">
        <AccountsSettings accounts={DEMO_ACCOUNTS} signingOut={undefined} />
      </CatalogExample>
      <CatalogExample label="Server unreachable">
        <AccountsSettings accounts={[]} error="Could not reach the Hot Sheet server." />
      </CatalogExample>
    </>
  );
}

export function SettingsWorkspaceDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'full' }}>
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
    </CatalogExample>
  );
}

export function TrashSettingsDemo() {
  return (
    <>
      <CatalogExample label="Default">
        <TrashSettings days={14} />
      </CatalogExample>
      {/* An unapplied shorter window enables "Apply now" and previews the purge (HS2-KPD942). */}
      <CatalogExample label="Unapplied change">
        <TrashSettings days={14} draft={{ value: '7', purgeCount: 3 }} />
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireSetupSettingsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-demo-ticket-source-scenario] [name="scenario"]', (_event, target) => {
      ticketSourceScenario.value = (target as FormControl).value as TicketSourceScenario;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="settings-workspace"] [name="workspace-category"]', (_event, target) => {
      settingsWorkspaceSettings.category.value = (target as FormControl).value as SettingsCategory;
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'change',
      '[data-settings="settings-workspace"] [name="workspace-permission-action"]',
      (_event, target) => {
        settingsWorkspaceSettings.permissionAction.value = (target as FormControl).value as PermissionAutomationAction;
      },
    ),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders: ticket-source setup and settings surfaces over the demo connection fixtures (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'keyboard-settings': { catalogExamples: true, Demo: KeyboardSettingsDemo },
    'ticket-source-setup-dialog': { catalogExamples: true, Demo: TicketSourceSetupDialogDemo },
    'provider-setup-form': { catalogExamples: true, Demo: ProviderSetupFormDemo },
    'ticket-sources-settings': { catalogExamples: true, Demo: TicketSourcesSettingsDemo },
    'accounts-settings': { catalogExamples: true, Demo: AccountsSettingsDemo },
    'settings-workspace': {
      catalogExamples: true,
      Demo: SettingsWorkspaceDemo,
      Settings: SettingsWorkspaceSettings,
      reset: resetSettingsWorkspaceDemo,
    },
    'trash-settings': { catalogExamples: true, Demo: TrashSettingsDemo },
  },
  wire: wireSetupSettingsDemo,
};
