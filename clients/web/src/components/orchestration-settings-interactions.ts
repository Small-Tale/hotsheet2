import { openPopupMenu } from '@kerfjs/ui/popup-menu';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { ProjectType } from '../api';
import type { OrchestrationSettings } from '../features/orchestration-settings';
import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';

const PROJECT_ROW = '[data-orchestration-project]';
const SESSION_ROW = '[data-orchestration-session]';
const ROW_MENU = '[data-component="popup-menu"]';
const PROJECT_TYPE = '[data-component="project-general-settings"] [name="project-type"]';
const POLICY_FIELD = '[data-component="orchestration-policy-settings"] [name]';
const LIMITS_FIELD = '[data-component="orchestration-project-limits"] [name]';

export interface OrchestrationSettingsWiring {
  /** The project whose settings show: the orchestration project. */
  projectId: () => string | undefined;
  /** Show an open project's tab. */
  activateProject: (projectId: string) => void;
}

/**
 * Delegated handlers for Project Settings → General → Project Type, Settings → Orchestration and a
 * project's detail in the inspector (HS2-W27N1D). Each handler runs one workflow of the controller;
 * the application and the catalog demos install the same handlers over the same controller.
 */
export function wireOrchestrationSettings(
  root: HTMLElement,
  settings: OrchestrationSettings,
  wiring: OrchestrationSettingsWiring,
): () => void {
  const lifetime = createScope();
  const withProject = (run: (projectId: string) => void) => {
    const projectId = wiring.projectId();
    if (projectId) run(projectId);
  };
  const openRowMenu = (row: Element | null) => {
    const menu = row?.querySelector<HTMLElement>(ROW_MENU);
    if (!menu) return false;
    openPopupMenu(menu);
    return true;
  };
  const attribute = (target: Element, name: string) => target.getAttribute(name) ?? undefined;
  lifetime.add(
    delegate(root, 'change', PROJECT_TYPE, (_event, target) => {
      const value = (target as HTMLInputElement).value;
      if (value === 'software' || value === 'orchestration')
        withProject((projectId) => void settings.setType(projectId, value satisfies ProjectType));
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.addOpenProject.selector, (_event, target) => {
      const id = attribute(target, 'data-project-id');
      if (id) withProject((projectId) => void settings.addOpenProject(projectId, id));
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.openAnotherProject.selector, () => {
      withProject((projectId) => {
        settings.openAnother(projectId);
      });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.openProjectDetail.selector, (_event, target) => {
      const id = attribute(target, 'data-item-id');
      if (id)
        withProject((projectId) => {
          settings.openDetail(projectId, id);
        });
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', `${PROJECT_ROW},${SESSION_ROW}`, (event, target) => {
      if (openRowMenu(target)) event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.openSessionActions.selector, (_event, target) => {
      openRowMenu(target.closest(SESSION_ROW));
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.showProjectTab.selector, (_event, target) => {
      const id = attribute(target, 'data-project-id');
      if (id) wiring.activateProject(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.removeProject.selector, (_event, target) => {
      const id = attribute(target, 'data-target-id');
      if (id) withProject((projectId) => void settings.removeProject(projectId, id));
    }),
  );
  for (const [spec, run] of [
    [ACTIONS.showSession, settings.showSession],
    [ACTIONS.takeOverSession, settings.takeOverSession],
    [ACTIONS.stopSession, settings.stopSession],
  ] as const)
    lifetime.add(
      delegate(root, 'click', spec.selector, (_event, target) => {
        const id = attribute(target, 'data-session-id');
        if (id) withProject((projectId) => void run(projectId, id));
      }),
    );
  lifetime.add(
    delegate(root, 'change', POLICY_FIELD, (_event, target) => {
      const control = target as HTMLInputElement;
      withProject((projectId) => {
        settings.changePolicy(projectId, control.name, control.type === 'checkbox' ? control.checked : control.value);
      });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.applyPolicy.selector, () => {
      withProject((projectId) => void settings.applyPolicy(projectId));
    }),
  );
  // HS2-7WF6JD: tool accounts in the policy form, and the selected project's Limits.
  lifetime.add(
    delegate(root, 'click', ACTIONS.addPolicyAccount.selector, (_event, target) => {
      const tool = attribute(target, 'data-tool');
      if (tool)
        withProject((projectId) => {
          settings.addAccount(projectId, tool);
        });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.removePolicyAccount.selector, (_event, target) => {
      const tool = attribute(target, 'data-tool'),
        index = Number(attribute(target, 'data-index'));
      if (tool && Number.isInteger(index))
        withProject((projectId) => {
          settings.removeAccount(projectId, tool, index);
        });
    }),
  );
  lifetime.add(
    delegate(root, 'change', LIMITS_FIELD, (_event, target) => {
      const control = target as HTMLInputElement;
      withProject((projectId) => {
        if (control.name === 'limits.tools' || control.name === 'limits.role_sessions')
          settings.changeLimits(projectId, control.name, control.value, control.checked);
        else
          settings.changeLimits(projectId, control.name, control.type === 'checkbox' ? control.checked : control.value);
      });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.applyProjectLimits.selector, () => {
      withProject((projectId) => void settings.applyLimits(projectId));
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
