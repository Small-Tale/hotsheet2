import { openAnchoredSurface } from '@kerfjs/ui/anchored-surface';
import { openPopupMenu } from '@kerfjs/ui/popup-menu';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { ProjectType } from '../api';
import type { OrchestrationSettings } from '../features/orchestration-settings';
import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import { ORCHESTRATION_PATTERN_COMMIT } from './app-combobox-wiring';
import { OrchestrationLimitsHelp } from './orchestration-limits-help';

const PROJECT_ROW = '[data-orchestration-project]';
const SESSION_ROW = '[data-orchestration-session]';
const ROW_MENU = '[data-component="popup-menu"]';
/** Kerf's `kui-menu-select`; cancelling it keeps the menu open. */
const POPUP_MENU_SELECT = 'kui-menu-select';
const PROJECT_TYPE = '[data-component="project-general-settings"] [name="project-type"]';
const POLICY_FIELD = '[data-component="orchestration-policy-settings"] [name]';
const LIMITS_FIELD = '[data-component="orchestration-project-limits"] [name]';
/** HS2-1X1TXC, HS2-NPAZQW: a Limits field of chosen models or resources. */
const PATTERN_FIELD = '[data-component="orchestration-pattern-field"]';
const PATTERN_INPUT = `${PATTERN_FIELD} input`;

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
    delegate(root, 'click', ACTIONS.addHostedProject.selector, (_event, target) => {
      const id = attribute(target, 'data-checkout-id');
      if (id) withProject((projectId) => void settings.addHostedProject(projectId, id));
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
  // HS2-E9VVQF: Add a Site on a project row, Separate from <project> on a site row.
  lifetime.add(
    delegate(root, 'click', ACTIONS.addSite.selector, (_event, target) => {
      const site = attribute(target, 'data-target-id'),
        open = attribute(target, 'data-project-id');
      if (site && open) withProject((projectId) => void settings.addSite(projectId, site, open));
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.showAllSites.selector, () => {
      withProject((projectId) => {
        settings.toggleAllSites(projectId);
      });
    }),
  );
  // Show All Projects changes the list it sits in, so the row's menu stays open to show the result. The
  // submenu itself closes on the re-render until Kerf keeps it open (KF-8QBJWN).
  lifetime.add(
    delegate(root, POPUP_MENU_SELECT, ROW_MENU, (event) => {
      const item = (event as CustomEvent<{ item?: Element }>).detail.item;
      if (item?.matches(ACTIONS.showAllSites.selector)) event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.openAnotherSite.selector, (_event, target) => {
      const site = attribute(target, 'data-target-id');
      if (site)
        withProject((projectId) => {
          settings.openAnother(projectId, site);
        });
    }),
  );
  // HS2-NAA3HV: Make <site> the Home Site on a site row.
  lifetime.add(
    delegate(root, 'click', ACTIONS.makeHomeSite.selector, (_event, target) => {
      const id = attribute(target, 'data-target-id');
      if (id) withProject((projectId) => void settings.makeHomeSite(projectId, id));
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.separateSite.selector, (_event, target) => {
      const id = attribute(target, 'data-target-id');
      if (id) withProject((projectId) => void settings.separateSite(projectId, id));
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
  // HS2-1X1TXC, HS2-NPAZQW: a model or resource chosen in, typed into, or removed from its field.
  const fieldOf = (target: Element) => target.closest<HTMLElement>(PATTERN_FIELD)?.dataset.limitsField;
  lifetime.add(
    delegate(root, ORCHESTRATION_PATTERN_COMMIT, PATTERN_INPUT, (event, target) => {
      const field = fieldOf(target);
      if (field)
        withProject((projectId) => {
          settings.editLimitsValue(projectId, field, (event as CustomEvent<string>).detail, true);
        });
    }),
  );
  lifetime.add(
    delegate(root, 'focusout', PATTERN_INPUT, (_event, target) => {
      const input = target as HTMLInputElement,
        field = fieldOf(input),
        typed = input.value.trim();
      if (!field || !typed) return;
      input.value = '';
      withProject((projectId) => {
        settings.editLimitsValue(projectId, field, typed, true);
      });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.removeLimitsValue.selector, (_event, target) => {
      const field = fieldOf(target),
        value = target.closest<HTMLElement>('[data-component="chip"]')?.dataset.itemId;
      if (field && value)
        withProject((projectId) => {
          settings.editLimitsValue(projectId, field, value, false);
        });
    }),
  );
  // HS2-Y737GM: what each limit means, beside the Limits header.
  lifetime.add(
    delegate(root, 'click', ACTIONS.showLimitsHelp.selector, (_event, target) => {
      openAnchoredSurface(target, OrchestrationLimitsHelp(), {
        label: 'About Limits',
        placement: 'bottom',
        align: 'end',
        dismiss: ['escape', 'outside'],
        initialFocus: false,
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
