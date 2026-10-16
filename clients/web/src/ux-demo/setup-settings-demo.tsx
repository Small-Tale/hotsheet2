import { CatalogExample } from '@kerfjs/ui/catalog';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { attr, delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { KeyboardSettings } from '../components/keyboard-settings';
import { PermissionRulesSettings } from '../components/permission-rules-settings';
import { ProviderSetupForm } from '../components/provider-setup-form';
import { type SettingsCategory } from '../components/settings-navigation';
import { SettingsWorkspace } from '../components/settings-workspace';
import { TicketSourceSetupDialog } from '../components/ticket-source-setup-dialog';
import { AccountsSettings, TicketSourcesSettings } from '../components/ticket-sources-settings';
import { trashDraftIsDirty, type TrashRetentionDraft, TrashSettings } from '../components/trash-settings';
import { wireTrashSettings } from '../components/trash-settings-interactions';
import { type PermissionAutomationAction } from '../permission-notifications';
import { type DemoDevice, DeviceSetting } from './components/device-setting';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { DEMO_ACCOUNTS, DEMO_PROJECT_SOURCES } from './fixtures';
import { syncSettingsControls } from './settings-controls';
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
/** Remote clients disable the host-only folder chooser (HS2-NWWF1Z). */
export const ticketSourceDevice = signal<DemoDevice>('host');

export function TicketSourceSetupDialogSettings() {
  return (
    <DemoSettingsForm settings="ticket-source-setup-dialog">
      <DeviceSetting value={ticketSourceDevice.value} />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </DemoSettingsForm>
  );
}

/** Restore the setup dialog's device setting (HS2-NWWF1Z). */
export function resetTicketSourceSetupDemo(root?: ParentNode): void {
  ticketSourceDevice.value = 'host';
  if (root) syncSettingsControls(root, 'ticket-source-setup-dialog', { values: { device: 'host' } });
}

export function KeyboardSettingsDemo() {
  return (
    <CatalogExample label="Apple Keyboard">
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
    <>
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
          hostActionsAvailable={ticketSourceDevice.value === 'host'}
          navigation="none"
        />
      </CatalogExample>
    </>
  );
}

export function ProviderSetupFormDemo() {
  return (
    <CatalogExample label="GitHub, Authorized">
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
    <CatalogExample label="Connected Sources">
      <TicketSourcesSettings sources={DEMO_PROJECT_SOURCES} />
    </CatalogExample>
  );
}

export function AccountsSettingsDemo() {
  return (
    <>
      <CatalogExample label="Signed-In Accounts">
        <AccountsSettings accounts={DEMO_ACCOUNTS} signingOut={undefined} />
      </CatalogExample>
      <CatalogExample label="Server Unreachable">
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

type TrashDemoExample = 'default' | 'unapplied' | 'leaving';
interface TrashDemoState {
  days: number;
  draft?: TrashRetentionDraft;
  message: string;
}
const TRASH_DEMO_INITIAL: Record<TrashDemoExample, TrashDemoState> = {
  default: { days: 14, message: '' },
  unapplied: { days: 14, draft: { value: '7', purgeCount: 3 }, message: '' },
  leaving: { days: 14, draft: { value: '7', purgeCount: 3 }, message: '' },
};
/** Per-example fixture retention state, driven through production's wireTrashSettings helper (HS2-WFFDAA). */
const trashDemo = signal<Record<TrashDemoExample, TrashDemoState>>(TRASH_DEMO_INITIAL);
const TRASH_DEMO_EXAMPLE = attr<'data-demo-trash', TrashDemoExample>('data-demo-trash');
const TRASH_DEMO_LEAVE = attr('data-action', 'demo-leave-trash-settings');

function trashDemoExample(element: Element): TrashDemoExample | undefined {
  return element.closest<HTMLElement>('[data-demo-trash]')?.dataset.demoTrash as TrashDemoExample | undefined;
}
function updateTrashDemo(element: Element, update: (state: TrashDemoState) => TrashDemoState) {
  const example = trashDemoExample(element);
  if (example) trashDemo.value = { ...trashDemo.value, [example]: update(trashDemo.value[example]) };
}
/** Fixture purge preview: the demo Trash holds 3 tickets older than 7 days and 5 older than 3. */
function trashDemoPurgeCount(value: string): number {
  const days = Number(value.trim());
  return !Number.isInteger(days) || days <= 0 ? 0 : days <= 3 ? 5 : days <= 7 ? 3 : 0;
}
function applyTrashDemo(state: TrashDemoState, value: string): TrashDemoState {
  const days = Number(value.trim());
  return Number.isInteger(days) && days > 0
    ? { days, message: '' }
    : { ...state, message: 'Enter a positive whole number of days.' };
}

const PERMISSION_RULES_DEMO = [
  {
    project: '/work/demo.hs2',
    connection: 'hotsheet-drawer-chat-1',
    tool: 'Bash',
    action: 'npm test',
    decision: 'allow',
    scope: 'chat',
  },
  { project: '/work/demo.hs2', tool: 'WebFetch', action: 'https://docs.rs', decision: 'allow', scope: 'session' },
  { project: '/work/demo.hs2', tool: 'Bash', action: 'rm -rf build', decision: 'deny', scope: 'always' },
] as const;

/** Remembered permission answers (HS2-JNHAYB): empty, listed, and a revoke in flight with an error. */
export function PermissionRulesSettingsDemo() {
  return (
    <>
      <CatalogExample label="No Remembered Answers">
        <PermissionRulesSettings rules={[]} />
      </CatalogExample>
      <CatalogExample label="Chat, Session, and Always Rules">
        <PermissionRulesSettings rules={PERMISSION_RULES_DEMO} />
      </CatalogExample>
      <CatalogExample label="Revoking, with a Failure Message">
        <PermissionRulesSettings
          rules={PERMISSION_RULES_DEMO}
          revoking={[JSON.stringify(['/work/demo.hs2', '', 'Bash', 'rm -rf build'])]}
          message="Could not revoke the rule. The server is unavailable."
        />
      </CatalogExample>
    </>
  );
}

export function resetTrashSettingsDemo() {
  trashDemo.value = TRASH_DEMO_INITIAL;
}

export function TrashSettingsDemo() {
  const state = trashDemo.value;
  return (
    <>
      <CatalogExample label="Default" rootAttributes={TRASH_DEMO_EXAMPLE('default')}>
        <TrashSettings {...state.default} />
      </CatalogExample>
      {/* An unapplied shorter window enables "Apply now" and previews the purge (HS2-KPD942). */}
      <CatalogExample label="Unapplied Change" rootAttributes={TRASH_DEMO_EXAMPLE('unapplied')}>
        <TrashSettings {...state.unapplied} />
      </CatalogExample>
      {/* Leaving Lifecycle with an unapplied value asks to apply or revert it (HS2-F55956, HS2-WFFDAA). */}
      <CatalogExample
        label="Leaving with an Unapplied Change"
        note="Leave Lifecycle stands in for choosing another settings category or project tab."
        rootAttributes={TRASH_DEMO_EXAMPLE('leaving')}
      >
        <List gap="m">
          <TrashSettings {...state.leaving} />
          <Row gap="xs">
            <wa-button appearance="outlined" type="button" {...TRASH_DEMO_LEAVE.attrs}>
              Leave Lifecycle
            </wa-button>
          </Row>
        </List>
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireSetupSettingsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    wireTrashSettings(root, {
      input: (value, field) => {
        updateTrashDemo(field, (state) => ({
          ...state,
          draft: { value, purgeCount: trashDemoPurgeCount(value) },
          message: '',
        }));
      },
      submit: (form) => {
        updateTrashDemo(form, (state) => applyTrashDemo(state, state.draft?.value ?? String(state.days)));
      },
      applyLeaving: (control) => {
        updateTrashDemo(control, (state) => applyTrashDemo(state, state.draft?.value ?? String(state.days)));
      },
      revert: (control) => {
        updateTrashDemo(control, (state) => ({ days: state.days, message: '' }));
      },
      dismissLeaving: (dialog) => {
        updateTrashDemo(dialog, (state) =>
          state.draft ? { ...state, draft: { ...state.draft, leaving: false } } : state,
        );
      },
    }),
  );
  // The stand-in for production's held navigation: leaving with a dirty draft raises the warning.
  demoListeners.add(
    delegate(root, 'click', TRASH_DEMO_LEAVE.selector, (_event, target) => {
      updateTrashDemo(target, (state) =>
        trashDraftIsDirty(state.days, state.draft) && state.draft
          ? { ...state, draft: { ...state.draft, leaving: true } }
          : state,
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ticket-source-setup-dialog"] [name="device"]', (_event, target) => {
      ticketSourceDevice.value = (target as FormControl).value as DemoDevice;
    }),
  );
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
    'ticket-source-setup-dialog': {
      catalogExamples: true,
      Demo: TicketSourceSetupDialogDemo,
      Settings: TicketSourceSetupDialogSettings,
      reset: resetTicketSourceSetupDemo,
    },
    'provider-setup-form': { catalogExamples: true, Demo: ProviderSetupFormDemo },
    'ticket-sources-settings': { catalogExamples: true, Demo: TicketSourcesSettingsDemo },
    'accounts-settings': { catalogExamples: true, Demo: AccountsSettingsDemo },
    'settings-workspace': {
      catalogExamples: true,
      Demo: SettingsWorkspaceDemo,
      Settings: SettingsWorkspaceSettings,
      reset: resetSettingsWorkspaceDemo,
    },
    'trash-settings': { catalogExamples: true, Demo: TrashSettingsDemo, reset: resetTrashSettingsDemo },
    'permission-rules-settings': { catalogExamples: true, Demo: PermissionRulesSettingsDemo },
  },
  wire: wireSetupSettingsDemo,
};
