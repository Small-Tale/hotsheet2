import { openPopupMenu } from '@kerfjs/ui/popup-menu';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { RegisteredProjects } from '../features/registered-projects';
import { NOTIFICATIONS_AND_LINKS_ACTIONS as ACTIONS } from '../interaction-attrs/notifications-and-links';

const ROW = '[data-registered-project]';
const ROW_MENU = '[data-component="popup-menu"]';

/**
 * Delegated handlers for App Settings → Projects (HS2-R806FZ). A row's primary button and a
 * right-click open the row's "…" menu; Remove from Hot Sheet runs one removal through the
 * controller, which then reloads the list. The application and the catalog demo install the same
 * handlers over the same controller.
 */
export function wireRegisteredProjectsSettings(root: HTMLElement, projects: RegisteredProjects): () => void {
  const lifetime = createScope();
  const openRowMenu = (row: Element | null) => {
    const menu = row?.querySelector<HTMLElement>(ROW_MENU);
    if (!menu) return false;
    openPopupMenu(menu);
    return true;
  };
  lifetime.add(
    delegate(root, 'click', ACTIONS.openRegisteredProjectActions.selector, (_event, target) => {
      openRowMenu(target.closest(ROW));
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', ROW, (event, target) => {
      if (openRowMenu(target)) event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.removeRegisteredProject.selector, (_event, target) => {
      const id = target.getAttribute('data-project-id');
      if (id) void projects.remove(id);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
