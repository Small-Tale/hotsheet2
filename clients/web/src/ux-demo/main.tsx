import '@kerfjs/ui/webawesome.css';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
// Kerf's Catalog composes a Workbench of Panes, Toolbars, and Lists and owns only its stage CSS, so
// the demo loads each composed primitive's stylesheet (Kerf 5.0.0-beta.55).
import '@kerfjs/ui/workbench.css';
import '@kerfjs/ui/pane.css';
import '@kerfjs/ui/toolbar.css';
import '@kerfjs/ui/text.css';
import '@kerfjs/ui/row.css';
import '@kerfjs/ui/list.css';
import '@kerfjs/ui/list-item.css';
import '@kerfjs/ui/list-header.css';
import '@kerfjs/ui/list-inset-text.css';
import '@kerfjs/ui/catalog.css';
import '@kerfjs/ui/floating-toolbar.css';
import '@kerfjs/ui/tab-bar.css';
import '@kerfjs/ui/select/register';
import '@kerfjs/ui/popup-menu/register';
import '@kerfjs/ui/document.css';
import '../hot-sheet-tokens.css';
import './style.css';

import { revealCatalogEntry, wireCatalog } from '@kerfjs/ui/wire-catalog';
import { wireToolbarVisibility } from '@kerfjs/ui/wire-toolbar-visibility';
import { delegate, mount } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { eventTargetsContextMenu } from '../components/ticket-row-context-menu';
import { type ContextPopupMenuElement, openContextPopupMenu } from '../context-menu-position';
import { withControlledOpen } from '../controlled-open';
import { wireTopLayerOverlays } from '../top-layer-overlay';
import { wireTicketRailToolbarVisibility, wireWorkspaceToolbarVisibility } from '../workspace-toolbar-visibility';
import { DemoApp } from './components/demo-app';
import {
  applyDisplayPreferences,
  backgroundStyle,
  collapsed,
  contextMenu,
  increasedContrast,
  reducedMotion,
  saveDisplayPreferences,
  selectDemo,
  selectedId,
  settingsCollapsed,
  tabContextMenu,
  theme,
  URL_PARAM,
  uxReviewProject,
} from './components/demo-state';
import { DEMO_ACTIONS } from './demo-actions';
import { listen } from './demo-module';
import { demoEntry, wireDemoModules } from './registry';

const root = document.querySelector<HTMLElement>('#ux-demo')!;
// Every delegated demo listener on `root` registers its disposer here, so the catalog's wiring has one
// teardown (`demoListeners.dispose()`) instead of hundreds of discarded disposers (HS2-G838PZ).
const demoListeners = createScope();
applyDisplayPreferences();
mount(root, withControlledOpen(root, DemoApp));
demoListeners.add(wireToolbarVisibility(root));
demoListeners.add(wireWorkspaceToolbarVisibility(root));
demoListeners.add(wireTicketRailToolbarVisibility(root));
// Every demo module installs its own interactions, in their original registration order.
demoListeners.add(wireDemoModules(root));
// Demo stages render context-mode PopupMenus statically; keep every one open so the catalog shows
// the menu itself (the app opens them from its own signals, HS2-2EHD8R).
const openStagedContextMenus = () => {
  for (const menu of root.querySelectorAll<ContextPopupMenuElement>('[data-context-menu]'))
    if (!menu.open) openContextPopupMenu(menu);
};
new MutationObserver(openStagedContextMenus).observe(root, { childList: true, subtree: true });
// Magnified terminals are manual popovers in the top layer, as in the app (HS2-Z9PQSC).
wireTopLayerOverlays(root);
openStagedContextMenus();
// The app clears its menu signals through capture-phase Escape and outside-press listeners; the
// demo's bubble-phase handlers run after Web Awesome has consumed the key, so mirror Web Awesome's
// own close (`wa-hide` on the menu root itself, not on a closing submenu) into the demo state.
demoListeners.add(
  delegate(root, 'wa-hide', '[data-context-menu]', (event, target) => {
    if (event.target !== target) return;
    const surface = (target as HTMLElement).dataset.contextMenu;
    // A selection hides the menu before its item click reaches the delegated handlers (microtasks flush
    // between listeners), so clear the state in a later task, after that click has finished dispatching.
    window.setTimeout(() => {
      if (surface === 'ticket') contextMenu.value = undefined;
      else if (surface === 'app-tab') tabContextMenu.value = undefined;
    }, 0);
  }),
);
// Kerf owns the catalog chrome: selection with `?component=` mirrored through replaceState, the sidebar
// (a transient overlay on a small screen through the app-owned `collapsed` flag), and the theme,
// contrast, reduced-motion, and preview-background controls (HS2-HMVBZW).
demoListeners.add(
  wireCatalog(root, {
    collapsed,
    settingsCollapsed,
    toggleSettingsAction: 'toggle-settings',
    onSelect: selectDemo,
    onUxReviewProject: (directory) => (uxReviewProject.value = directory),
    onToggleSidebar: () => {
      collapsed.value = !collapsed.value;
      saveDisplayPreferences();
    },
    onToggleTheme: () => {
      theme.value = theme.value === 'dark' ? 'light' : 'dark';
      applyDisplayPreferences();
      saveDisplayPreferences();
    },
    onToggleContrast: () => {
      increasedContrast.value = !increasedContrast.value;
      applyDisplayPreferences();
      saveDisplayPreferences();
    },
    onToggleMotion: () => {
      reducedMotion.value = !reducedMotion.value;
      applyDisplayPreferences();
      saveDisplayPreferences();
    },
    onSelectBackground: (style) => {
      backgroundStyle.value = style;
      saveDisplayPreferences();
    },
    urlParam: URL_PARAM,
    revealSelection: true,
  }),
);
revealCatalogEntry(root, selectedId.value, { block: 'center' });
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.resetSettings.selector, () => {
    demoEntry(selectedId.value)?.reset?.(root);
  }),
);
demoListeners.add(
  listen(
    window,
    'pointerdown',
    (event) => {
      if (contextMenu.value && !eventTargetsContextMenu(event)) contextMenu.value = undefined;
      if (tabContextMenu.value && !eventTargetsContextMenu(event, '[data-component="app-tab-context-menu"]'))
        tabContextMenu.value = undefined;
    },
    { capture: true },
  ),
);
demoListeners.add(
  listen(window, 'keydown', (event) => {
    if (event.key === 'Escape') contextMenu.value = undefined;
  }),
);
