import { AppTab } from '@kerfjs/ui/app-tab';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { Activity, FolderGit2, Minus, Plus, Terminal } from 'lucide';

import { AppEmptyState, AppMessageState, ProjectRestoreState } from '../components/app-empty-state';
import { AppError } from '../components/app-error';
import { CodexHooksNoticeBanner } from '../components/codex-hooks-notice-banner';
import { ProjectSetupWarningBanner } from '../components/project-setup-warning-banner';
import { ShellBanner } from '../components/shell-banner';
import { TicketPageMore } from '../components/ticket-page-more';
import type { DemoModule } from './demo-module';

export const appErrorDemoVisible = signal(true);

export function FloatingToolbarDemo() {
  return (
    <CatalogExample
      label="Over content"
      note="Floating controls remain available over scrolling content without joining the page toolbar."
      viewport={{
        width: 'full',
        minHeight: 'short',
        surface: 'lowered',
        tokens: { '--kui-color-neutral-fill-normal': 'var(--hs-floating-control-surface)' },
      }}
    >
      <FloatingToolbar label="Preview zoom controls" position="bottom-end">
        <ToolbarControlGroup>
          <button type="button" aria-label="Zoom out">
            <LucideIcon icon={Minus} name="minus" />
          </button>
          <button type="button" aria-label="Zoom in">
            <LucideIcon icon={Plus} name="plus" />
          </button>
        </ToolbarControlGroup>
      </FloatingToolbar>
    </CatalogExample>
  );
}

export function AppTabDemo() {
  return (
    <CatalogExample
      label="Project and terminal tabs"
      viewport={{ width: 'full', layout: 'flex', frame: 'solid', surface: 'lowered' }}
    >
      <TabBar id="app-tab-demo" label="Shared application tab demo">
        <AppTab
          id="project"
          name="Project tab"
          selected
          nameOverflow="visible"
          rootAttributes={{ 'data-tab-kind': 'project', 'data-project-id': 'project' }}
          leading={<LucideIcon icon={FolderGit2} name="folder-git-2" />}
        />
        <AppTab
          id="terminal"
          name="Terminal tab"
          className="terminal-tab"
          rootAttributes={{ 'data-tab-kind': 'terminal', 'data-terminal-id': 'terminal' }}
          leading={<LucideIcon icon={Terminal} name="terminal" />}
          trailing={
            <span aria-label="Busy">
              <LucideIcon icon={Activity} name="activity" />
            </span>
          }
        />
      </TabBar>
    </CatalogExample>
  );
}

export function CodexHooksNoticeBannerDemo() {
  return (
    <CatalogExample label="Default">
      <CodexHooksNoticeBanner path=".codex/hooks.json" />
    </CatalogExample>
  );
}

export function ProjectSetupWarningBannerDemo() {
  return (
    <CatalogExample label="Default">
      <ProjectSetupWarningBanner detail="The development Hot Sheet CLI does not match the current setup templates and may overwrite newer project guidance. Run cargo build -p hotsheet-cli, then reopen the project. No setup files were changed." />
    </CatalogExample>
  );
}

export function ShellBannerDemo() {
  return (
    <>
      <CatalogExample label="Info with trailing action">
        <ShellBanner
          component="shell-banner-demo"
          tone="info"
          title="Information strip"
          detail="Info tone with a trailing action."
          action={<button type="button">Action</button>}
        />
      </CatalogExample>
      <CatalogExample label="Success with action below">
        <ShellBanner
          component="shell-banner-demo"
          tone="success"
          title="Success strip"
          detail="Success tone with the action below the copy."
          actionPlacement="below"
          action={<button type="button">Dismiss</button>}
        />
      </CatalogExample>
      <CatalogExample label="Warning without action">
        <ShellBanner
          component="shell-banner-demo"
          tone="warning"
          title="Warning strip"
          detail="Warning tone, no action."
        />
      </CatalogExample>
    </>
  );
}

export function TicketPageMoreDemo() {
  return (
    <>
      <CatalogExample label="More available">
        <TicketPageMore />
      </CatalogExample>
      <CatalogExample label="Loading">
        <TicketPageMore loading />
      </CatalogExample>
    </>
  );
}

export function AppEmptyStateDemo() {
  return (
    <>
      <CatalogExample label="No project open">
        <AppEmptyState />
      </CatalogExample>
      <CatalogExample label="Restoring">
        <ProjectRestoreState />
      </CatalogExample>
      <CatalogExample label="Message">
        <AppMessageState title="Project unavailable" message="Open another project to continue working." />
      </CatalogExample>
    </>
  );
}

export function AppErrorDemo() {
  return (
    <CatalogExample label="Interactive">
      <button type="button" data-app-error-demo-show>
        Show error
      </button>
      {appErrorDemoVisible.value && <AppError message="The project could not be opened. Try again." />}
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireAppChromeDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'click', '[data-app-error-demo-show]', () => {
      appErrorDemoVisible.value = true;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-component="app-error"] [data-action="dismiss-app-error"]', () => {
      appErrorDemoVisible.value = false;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders: small application chrome and feedback surfaces (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'floating-toolbar': { catalogExamples: true, Demo: FloatingToolbarDemo },
    'app-tab': { catalogExamples: true, Demo: AppTabDemo },
    'codex-hooks-notice-banner': { catalogExamples: true, Demo: CodexHooksNoticeBannerDemo },
    'shell-banner': { catalogExamples: true, Demo: ShellBannerDemo },
    'project-setup-warning-banner': { catalogExamples: true, Demo: ProjectSetupWarningBannerDemo },
    'ticket-page-more': { catalogExamples: true, Demo: TicketPageMoreDemo },
    'app-empty-state': { catalogExamples: true, Demo: AppEmptyStateDemo },
    'app-error': { catalogExamples: true, Demo: AppErrorDemo },
  },
  wire: wireAppChromeDemo,
};
