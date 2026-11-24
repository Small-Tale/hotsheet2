import { effect, type Signal } from 'kerfjs';

import { Api } from '../api';
import { orchestrationProjectPanel } from '../components/orchestration-project-detail';
import type { OrchestrationSettingsProps } from '../components/orchestration-settings';
import type { ProjectGeneralSettingsProps } from '../components/project-general-settings';
import type { SettingsCategory } from '../components/settings-navigation';
import type { SidebarPanelParts } from '../components/sidebar-panel';
import type { Choose, Confirm } from '../confirmation';
import type { Project } from '../interactions/types';
import {
  addChoices,
  projectRows,
  resourceSuggestions,
  siteChoicesByProject,
  siteReadFailure,
  usablePolicyTools,
} from '../orchestration-settings-model';
import { isRemoteProjectId } from '../remote-projects/identity';
import { createOrchestrationSettings } from './orchestration-settings';

/** Live application bindings Settings → General and Settings → Orchestration read (HS2-W27N1D). */
export interface OrchestrationSettingsSurfaceDependencies {
  projects: { readonly value: readonly Project[] };
  project: () => Project | undefined;
  viewMode: { readonly value: string };
  selectedSettingsCategory: Signal<SettingsCategory>;
  confirm: Confirm;
  /** HS2-E9VVQF: + Add Project's duplicate question. */
  choose?: Choose;
  openProjectPicker: () => void;
  openFlowBusy: () => boolean;
  /** Show an open project's tab. */
  selectProjectTab: (projectId: string) => void;
  /** Show the Settings view of the current tab. */
  showSettings: () => void;
  /** Show an open project's terminal in its drawer. */
  openTerminalInProject: (key: string) => void;
  hasTerminal: (key: string) => boolean;
  /** HS2-1X1TXC: each tool's reported models feed its Models control. */
  aiTools: () => readonly { id: string; models?: readonly { id: string }[] }[];
  toolLabel: (tool: string) => string;
  showInspector: () => void;
  /** Reviewed remote server names by registration id. */
  remoteServerLabels: () => Promise<Record<string, string>>;
}

/**
 * Wires the Project Type and Orchestration settings controller into the application: what it reads
 * while a category shows, the categories the navigator lists, and the props of the workspace and the
 * inspector. Reads happen when a category shows and on the project's `orchestration_changed` events.
 */
export function createOrchestrationSettingsSurface(dependencies: OrchestrationSettingsSurfaceDependencies) {
  const { projects, project, viewMode, selectedSettingsCategory } = dependencies;
  const settings = createOrchestrationSettings({
    api: (projectId) => {
      const open = projects.value.find((item) => item.id === projectId);
      return open ? new Api(open.apiPath, '') : undefined;
    },
    openProjects: () => projects.value.map((item) => ({ id: item.id, name: item.name })),
    confirm: dependencies.confirm,
    choose: dependencies.choose,
    openProjectPicker: dependencies.openProjectPicker,
    openFlowBusy: dependencies.openFlowBusy,
    activateProject: (projectId) => {
      // Open Another Project… returns here: the orchestration project's settings, where the new row shows.
      dependencies.selectProjectTab(projectId);
      dependencies.showSettings();
      selectedSettingsCategory.value = 'orchestration';
    },
    showTerminal: (projectId, terminalId) => {
      if (!dependencies.hasTerminal(`${projectId}:${terminalId}`)) return false;
      dependencies.openTerminalInProject(`${projectId}:${terminalId}`);
      return true;
    },
    hasTerminal: (projectId, terminalId) => dependencies.hasTerminal(`${projectId}:${terminalId}`),
    knownTools: () => dependencies.aiTools().map((tool) => tool.id),
    showInspector: dependencies.showInspector,
  });

  /**
   * The project's type: the latest read here, else the one read while the project opened (HS2-AMCSKS),
   * so the Orchestration category and view are in place on the first paint. `undefined` when neither.
   */
  const knownOrchestrationProject = (projectId: string): boolean | undefined => {
    const type =
      settings.types.value[projectId]?.view?.project_type ??
      projects.value.find((item) => item.id === projectId)?.projectType;
    return type ? type === 'orchestration' : undefined;
  };
  const orchestrationProject = (projectId: string | undefined) =>
    Boolean(projectId && knownOrchestrationProject(projectId));

  /** The category Settings shows: Orchestration falls back to General once the project is Software. */
  function settingsCategory(): SettingsCategory {
    const selected = selectedSettingsCategory.value;
    return selected === 'orchestration' && !orchestrationProject(project()?.id) ? 'project-general' : selected;
  }

  let typeKey = '';
  effect(() => {
    const current = project(),
      key = current && viewMode.value === 'settings' ? `${current.id}:${current.apiPath}` : '';
    if (key === typeKey) return;
    typeKey = key;
    if (current && key) void settings.loadType(current.id);
  });
  let orchestrationKey = '';
  effect(() => {
    const current = project(),
      key =
        current &&
        viewMode.value === 'settings' &&
        selectedSettingsCategory.value === 'orchestration' &&
        orchestrationProject(current.id)
          ? `${current.id}:${current.apiPath}`
          : '';
    if (key === orchestrationKey) return;
    orchestrationKey = key;
    if (current && key) {
      void settings.loadSettings(current.id);
      void settings.loadRemoteLabels(dependencies.remoteServerLabels);
    }
  });
  effect(() => {
    dependencies.openFlowBusy();
    settings.noteOpenFlow();
  });

  /** The project's `orchestration_changed` event, an orchestrated session's change, or a resync. */
  function changed(current: Project) {
    const showing = viewMode.value === 'settings' && project()?.id === current.id;
    settings.changed(current.id, {
      type: showing,
      orchestration: showing && settingsCategory() === 'orchestration',
    });
  }

  function projectGeneral(projectId: string): ProjectGeneralSettingsProps {
    const state = settings.types.value[projectId];
    return {
      projectType: state?.view?.project_type,
      activeProjects: state?.view?.active_projects,
      loading: state?.loading,
      saving: state?.saving,
      message: state?.message,
    };
  }

  /** HS2-DXAB4K: the server a project open from another server is served by, as + Add Project names it. */
  function servedBy(current: Project): string | undefined {
    return isRemoteProjectId(current.id) ? (current.remoteHost ?? 'the server') : undefined;
  }

  function orchestration(current: Project): OrchestrationSettingsProps {
    const state = settings.settings.value[current.id],
      labels = settings.remoteLabels.value,
      rows = projectRows(state?.manifest, state?.bindings ?? {}, labels, state?.sites, servedBy(current)),
      choices = addChoices(
        projects.value.map((item) => ({ id: item.id, name: item.name })),
        current.id,
        state?.bindings ?? {},
        state?.manifest,
        labels,
      );
    return {
      rows,
      choices,
      // HS2-E9VVQF: a project served here offers Add a Site on each project row.
      ...(isRemoteProjectId(current.id)
        ? {}
        : {
            siteChoices: siteChoicesByProject(
              rows,
              choices,
              state?.stores ?? {},
              state?.manifest,
              state?.bindings ?? {},
              Boolean(state?.showAllSites),
            ),
            showAllSites: Boolean(state?.showAllSites),
          }),
      openTabs: projects.value.map((item) => item.id),
      selectedTarget: settings.selected.value[current.id],
      loading: state?.loading ?? true,
      busy: state?.busy,
      message: state?.message,
      status: state?.status,
      // HS2-92FJXM: a project open from another server adds that server's open projects.
      ...(isRemoteProjectId(current.id)
        ? {
            hostedChoices: (state?.hosted ?? []).map((choice) => ({
              checkoutId: choice.checkout_id,
              name: choice.name,
              added: choice.added,
            })),
            hostedServer: current.remoteHost ?? 'the server',
            unavailableReason:
              state?.hosted === undefined && state?.hostedError
                ? `Could not read the projects open on ${current.remoteHost ?? 'the server'}. ${state.hostedError}`
                : undefined,
          }
        : {}),
      policy: { draft: state?.draft, toolLabel: dependencies.toolLabel },
    };
  }

  /** The inspector while Settings → Orchestration shows a selected project. */
  function inspector(current: Project): SidebarPanelParts | undefined {
    if (settingsCategory() !== 'orchestration') return undefined;
    const target = settings.selected.value[current.id];
    if (!target) return undefined;
    const state = settings.settings.value[current.id],
      rows = projectRows(
        state?.manifest,
        state?.bindings ?? {},
        settings.remoteLabels.value,
        undefined,
        servedBy(current),
      ),
      row = rows.find((item) => item.targetId === target),
      // HS2-D4G81Q: every site of the target's project, with what each machine is.
      siteProject = state?.sites?.projects.find((item) => item.sites.some((site) => site.target_id === target)),
      detail = settings.details.value[`${current.id}:${target}`],
      policy = state?.policy?.policy;
    if (!row) return undefined;
    return orchestrationProjectPanel({
      targetId: target,
      label: row.label,
      where: row.where,
      binding: state?.bindings[target],
      detail,
      sessions: (detail?.sessions ?? []).map((session) => ({
        session,
        showable: Boolean(settings.sessionTab(current.id, session)),
        busy: Boolean(state?.busy.includes(session.session_id)),
      })),
      claimNamespaces: [
        ...new Set(
          (policy?.placement?.resources ?? [])
            .filter((entry) => entry.target_id === target)
            .flatMap((entry) => entry.resources.map((resource) => resource.namespace)),
        ),
      ],
      removing: Boolean(state?.busy.includes(target)),
      policyTools: usablePolicyTools(
        policy,
        dependencies.aiTools().map((tool) => tool.id),
      ),
      // HS2-SYZB33: another site's coordination sessions start on its project's home.
      homeLabel: row.siteOf ? rows.find((item) => item.targetId === row.siteOf)?.label : undefined,
      sites: (siteProject?.sites ?? []).map((site) => ({
        targetId: site.target_id,
        label: site.label,
        where: rows.find((item) => item.targetId === site.target_id)?.place ?? '',
        home: site.project,
        capabilities: site.capabilities,
        error: site.capabilities_error ? siteReadFailure(site.capabilities_error) : undefined,
      })),
      // HS2-1X1TXC: the models each tool reports here, for its Models control.
      toolModels: Object.fromEntries(
        dependencies.aiTools().map((tool) => [tool.id, (tool.models ?? []).map((model) => model.id)]),
      ),
      // HS2-NPAZQW: the declared resources (the policy's and those held here) and their patterns.
      resourceChoices: resourceSuggestions(policy, detail?.capacity?.claims ?? [], []),
      toolLabel: dependencies.toolLabel,
    });
  }

  return {
    settings,
    settingsCategory,
    orchestrationProject,
    knownOrchestrationProject,
    changed,
    projectGeneral,
    orchestration,
    inspector,
  };
}

export type OrchestrationSettingsSurface = ReturnType<typeof createOrchestrationSettingsSurface>;
