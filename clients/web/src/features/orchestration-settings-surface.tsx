import { effect, type Signal } from 'kerfjs';

import { Api } from '../api';
import { orchestrationProjectPanel } from '../components/orchestration-project-detail';
import type { OrchestrationSettingsProps } from '../components/orchestration-settings';
import type { ProjectGeneralSettingsProps } from '../components/project-general-settings';
import type { SettingsCategory } from '../components/settings-navigation';
import type { SidebarPanelParts } from '../components/sidebar-panel';
import type { Confirm } from '../confirmation';
import type { Project } from '../interactions/types';
import { addChoices, projectRows } from '../orchestration-settings-model';
import { createOrchestrationSettings } from './orchestration-settings';

/** Live application bindings Settings → General and Settings → Orchestration read (HS2-W27N1D). */
export interface OrchestrationSettingsSurfaceDependencies {
  projects: { readonly value: readonly Project[] };
  project: () => Project | undefined;
  viewMode: { readonly value: string };
  selectedSettingsCategory: Signal<SettingsCategory>;
  confirm: Confirm;
  openProjectPicker: () => void;
  openFlowBusy: () => boolean;
  /** Show an open project's tab. */
  selectProjectTab: (projectId: string) => void;
  /** Show the Settings view of the current tab. */
  showSettings: () => void;
  /** Show an open project's terminal in its drawer. */
  openTerminalInProject: (key: string) => void;
  hasTerminal: (key: string) => boolean;
  aiTools: () => readonly { id: string }[];
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

  const orchestrationProject = (projectId: string | undefined) =>
    Boolean(projectId && settings.types.value[projectId]?.view?.project_type === 'orchestration');
  /** The same answer, `undefined` until this project's type was read (the view then reads its own). */
  const knownOrchestrationProject = (projectId: string): boolean | undefined => {
    const view = settings.types.value[projectId]?.view;
    return view ? view.project_type === 'orchestration' : undefined;
  };

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

  function orchestration(current: Project): OrchestrationSettingsProps {
    const state = settings.settings.value[current.id],
      labels = settings.remoteLabels.value;
    return {
      rows: projectRows(state?.manifest, state?.bindings ?? {}, labels),
      choices: addChoices(
        projects.value.map((item) => ({ id: item.id, name: item.name })),
        current.id,
        state?.bindings ?? {},
        state?.manifest,
        labels,
      ),
      openTabs: projects.value.map((item) => item.id),
      selectedTarget: settings.selected.value[current.id],
      loading: state?.loading ?? true,
      busy: state?.busy,
      message: state?.message,
      status: state?.status,
      unavailableReason: current.id.startsWith('remote:')
        ? 'This project is open from another server. Add projects to it on the computer it runs on.'
        : undefined,
      policy: { draft: state?.draft, toolLabel: dependencies.toolLabel },
    };
  }

  /** The inspector while Settings → Orchestration shows a selected project. */
  function inspector(current: Project): SidebarPanelParts | undefined {
    if (settingsCategory() !== 'orchestration') return undefined;
    const target = settings.selected.value[current.id];
    if (!target) return undefined;
    const state = settings.settings.value[current.id],
      row = projectRows(state?.manifest, state?.bindings ?? {}, settings.remoteLabels.value).find(
        (item) => item.targetId === target,
      ),
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
      policyTools: (policy?.tools ?? []).map((tool) => tool.tool),
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
