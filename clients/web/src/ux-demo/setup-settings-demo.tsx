import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { attr, delegate, delegateCapture, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { KeyboardSettings } from '../components/keyboard-settings';
import { permissionRulesPage, PermissionRulesSettings } from '../components/permission-rules-settings';
import { ProviderSetupForm } from '../components/provider-setup-form';
import { type SettingsCategory } from '../components/settings-navigation';
import { SettingsWorkspace } from '../components/settings-workspace';
import { TerminalMemorySettings } from '../components/terminal-memory-settings';
import { TicketSourceSetupDialog } from '../components/ticket-source-setup-dialog';
import { AccountsSettings, TicketSourcesSettings } from '../components/ticket-sources-settings';
import { trashDraftIsDirty, type TrashRetentionDraft, TrashSettings } from '../components/trash-settings';
import { wireTrashSettings } from '../components/trash-settings-interactions';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { type PermissionAutomationAction, type PermissionRule, permissionRuleKey } from '../permission-notifications';
import { type DemoDevice, DeviceSetting } from './components/device-setting';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { DEMO_ACCOUNTS, DEMO_PROJECT_SOURCES, DEMO_TERMINAL_LIMITS, DEMO_TERMINAL_LIMITS_CONFIGURED } from './fixtures';
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
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
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
        terminalMemory={{ limits: DEMO_TERMINAL_LIMITS }}
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
    action: `hmac-sha256:v2:${'a'.repeat(16)}:${'a'.repeat(56)}12345678`,
    decision: 'allow',
    scope: 'chat',
  },
  {
    project: '/work/demo.hs2',
    tool: 'WebFetch',
    action: `hmac-sha256:v2:${'b'.repeat(16)}:${'b'.repeat(56)}87654321`,
    decision: 'allow',
    scope: 'session',
  },
  {
    project: '/work/demo.hs2',
    tool: 'Bash',
    action: `hmac-sha256:v2:${'c'.repeat(16)}:${'c'.repeat(56)}feedcafe`,
    decision: 'deny',
    scope: 'always',
  },
  {
    project: '/work/demo.hs2',
    tool: 'Bash',
    action: `hmac-sha256:v2:${'d'.repeat(16)}:${'d'.repeat(56)}00000004`,
    decision: 'allow',
    scope: 'always',
  },
  {
    project: '/work/demo.hs2',
    tool: 'WebFetch',
    action: `hmac-sha256:v2:${'e'.repeat(16)}:${'e'.repeat(56)}00000005`,
    decision: 'deny',
    scope: 'session',
  },
  {
    project: '/work/demo.hs2',
    tool: 'Bash',
    action: `hmac-sha256:v2:${'f'.repeat(16)}:${'f'.repeat(56)}00000006`,
    decision: 'allow',
    scope: 'chat',
  },
] as const;

const permissionRulesDemoDialog = signal<
  | {
      query: string;
      page: number;
      selected?: readonly string[];
      confirmingDelete?: boolean;
    }
  | undefined
>(undefined);
const permissionRulesDemoRules = signal<PermissionRule[]>([...PERMISSION_RULES_DEMO]);

/** Remembered permission answers (HS2-JNHAYB): empty, listed, and a revoke in flight with an error. */
export function PermissionRulesSettingsDemo() {
  return (
    <>
      <CatalogExample label="No Remembered Answers">
        <PermissionRulesSettings rules={[]} />
      </CatalogExample>
      <CatalogExample label="Chat, Session, and Always Rules">
        <div data-demo-permission-rules>
          <PermissionRulesSettings rules={permissionRulesDemoRules.value} dialog={permissionRulesDemoDialog.value} />
        </div>
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

/** App Settings → Terminal memory (HS2-Y5DYC0): every state the settings page renders. */
export function TerminalMemorySettingsDemo() {
  return (
    <>
      <CatalogExample label="Memory-Derived Defaults">
        <TerminalMemorySettings limits={DEMO_TERMINAL_LIMITS} />
      </CatalogExample>
      {/* A value set from the CLI outside the presets is still listed and selected. */}
      <CatalogExample label="Explicit Limits">
        <TerminalMemorySettings limits={DEMO_TERMINAL_LIMITS_CONFIGURED} />
      </CatalogExample>
      <CatalogExample label="Another Device">
        <TerminalMemorySettings
          limits={DEMO_TERMINAL_LIMITS}
          deviceDefaults={{ historyLines: 5_000, warmTerminals: 12 }}
        />
      </CatalogExample>
      <CatalogExample label="Unknown Memory">
        <TerminalMemorySettings limits={{ ...DEMO_TERMINAL_LIMITS, physical_memory_bytes: null }} />
      </CatalogExample>
      <CatalogExample label="Loading">
        <TerminalMemorySettings />
      </CatalogExample>
      <CatalogExample label="Save Failed">
        <TerminalMemorySettings
          limits={DEMO_TERMINAL_LIMITS}
          message="terminal.history_lines must be a whole number from 100 to 100000"
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
            <Button emphasis="secondary" type="button" action={TRASH_DEMO_LEAVE.value}>
              Leave Lifecycle
            </Button>
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
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-demo-permission-rules] ${NOTIFICATIONS_AND_LINKS_ACTIONS.openPermissionRules.selector}`,
      () => {
        permissionRulesDemoDialog.value = { query: '', page: 1, selected: [] };
      },
    ),
  );
  demoListeners.add(
    delegate(root, 'input', '[data-demo-permission-rules] [name="permission-rule-search"]', (_event, target) => {
      permissionRulesDemoDialog.value = {
        ...permissionRulesDemoDialog.value,
        query: (target as FormControl).value,
        page: 1,
        confirmingDelete: false,
      };
    }),
  );
  for (const [selector, offset] of [
    [NOTIFICATIONS_AND_LINKS_ACTIONS.previousPermissionRulesPage.selector, -1],
    [NOTIFICATIONS_AND_LINKS_ACTIONS.nextPermissionRulesPage.selector, 1],
  ] as const)
    demoListeners.add(
      delegate(root, 'click', `[data-demo-permission-rules] ${selector}`, () => {
        const state = permissionRulesDemoDialog.value;
        if (state)
          permissionRulesDemoDialog.value = {
            ...state,
            page: permissionRulesPage(permissionRulesDemoRules.value, state.query, state.page + offset).page,
          };
      }),
    );
  demoListeners.add(
    delegate(
      root,
      'change',
      `[data-demo-permission-rules] ${NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRule.selector}`,
      (_event, target) => {
        const state = permissionRulesDemoDialog.value;
        if (!state) return;
        const selected = new Set(state.selected ?? []),
          key = (target as HTMLElement).getAttribute('data-rule-key');
        if (!key) return;
        if ((target as FormControl & { checked: boolean }).checked) selected.add(key);
        else selected.delete(key);
        permissionRulesDemoDialog.value = { ...state, selected: [...selected], confirmingDelete: false };
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-demo-permission-rules] ${NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRulesPage.selector}`,
      () => {
        const state = permissionRulesDemoDialog.value;
        if (!state) return;
        const selected = new Set(state.selected ?? []),
          rows = permissionRulesPage(permissionRulesDemoRules.value, state.query, state.page).rows,
          all = rows.every((rule) => selected.has(permissionRuleKey(rule)));
        for (const rule of rows) {
          const key = permissionRuleKey(rule);
          if (all) selected.delete(key);
          else selected.add(key);
        }
        permissionRulesDemoDialog.value = { ...state, selected: [...selected], confirmingDelete: false };
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-demo-permission-rules] ${NOTIFICATIONS_AND_LINKS_ACTIONS.clearPermissionRuleSelection.selector}`,
      () => {
        const state = permissionRulesDemoDialog.value;
        if (state) permissionRulesDemoDialog.value = { ...state, selected: [], confirmingDelete: false };
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-demo-permission-rules] ${NOTIFICATIONS_AND_LINKS_ACTIONS.editSelectedPermissionRules.selector}`,
      (_event, target) => {
        const state = permissionRulesDemoDialog.value,
          decision = (target as HTMLElement).getAttribute('data-decision');
        if (!state || (decision !== 'allow' && decision !== 'deny')) return;
        const selected = new Set(state.selected ?? []);
        permissionRulesDemoRules.value = permissionRulesDemoRules.value.map((rule) =>
          selected.has(permissionRuleKey(rule)) ? { ...rule, decision } : rule,
        );
        permissionRulesDemoDialog.value = { ...state, selected: [] };
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-demo-permission-rules] ${NOTIFICATIONS_AND_LINKS_ACTIONS.requestDeleteSelectedPermissionRules.selector}`,
      () => {
        const state = permissionRulesDemoDialog.value;
        if (state) permissionRulesDemoDialog.value = { ...state, confirmingDelete: true };
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-demo-permission-rules] ${NOTIFICATIONS_AND_LINKS_ACTIONS.cancelDeleteSelectedPermissionRules.selector}`,
      () => {
        const state = permissionRulesDemoDialog.value;
        if (state) permissionRulesDemoDialog.value = { ...state, confirmingDelete: false };
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-demo-permission-rules] ${NOTIFICATIONS_AND_LINKS_ACTIONS.confirmDeleteSelectedPermissionRules.selector}`,
      () => {
        const state = permissionRulesDemoDialog.value;
        if (!state?.confirmingDelete) return;
        const selected = new Set(state.selected ?? []);
        permissionRulesDemoRules.value = permissionRulesDemoRules.value.filter(
          (rule) => !selected.has(permissionRuleKey(rule)),
        );
        permissionRulesDemoDialog.value = {
          ...state,
          selected: [],
          confirmingDelete: false,
          page: permissionRulesPage(permissionRulesDemoRules.value, state.query, state.page).page,
        };
      },
    ),
  );
  demoListeners.add(
    delegateCapture(
      root,
      'kui-request-close',
      '[data-demo-permission-rules] [data-dialog-component="permission-rules-dialog"]',
      () => {
        permissionRulesDemoDialog.value = undefined;
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
    'terminal-memory-settings': { catalogExamples: true, Demo: TerminalMemorySettingsDemo },
    'permission-rules-settings': {
      catalogExamples: true,
      Demo: PermissionRulesSettingsDemo,
      reset: () => {
        permissionRulesDemoDialog.value = undefined;
      },
    },
  },
  wire: wireSetupSettingsDemo,
};
