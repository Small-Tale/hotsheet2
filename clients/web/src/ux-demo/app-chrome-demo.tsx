import { AppTab } from '@kerfjs/ui/app-tab';
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
    <section class="floating-toolbar-demo">
      <p class="floating-toolbar-demo__copy">
        Floating controls remain available over scrolling content without joining the page toolbar.
      </p>
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
    </section>
  );
}

export function AppTabDemo() {
  return (
    <section class="app-tab-demo">
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
    </section>
  );
}

export function CodexHooksNoticeBannerDemo() {
  return <CodexHooksNoticeBanner path=".codex/hooks.json" />;
}

export function ProjectSetupWarningBannerDemo() {
  return (
    <ProjectSetupWarningBanner detail="The development Hot Sheet CLI does not match the current setup templates and may overwrite newer project guidance. Run cargo build -p hotsheet-cli, then reopen the project. No setup files were changed." />
  );
}

export function ShellBannerDemo() {
  return (
    <section aria-label="Shell banner tones">
      <ShellBanner
        component="shell-banner-demo"
        tone="info"
        title="Information strip"
        detail="Info tone with a trailing action."
        action={<button type="button">Action</button>}
      />
      <ShellBanner
        component="shell-banner-demo"
        tone="success"
        title="Success strip"
        detail="Success tone with the action below the copy."
        actionPlacement="below"
        action={<button type="button">Dismiss</button>}
      />
      <ShellBanner
        component="shell-banner-demo"
        tone="warning"
        title="Warning strip"
        detail="Warning tone, no action."
      />
    </section>
  );
}

export function TicketPageMoreDemo() {
  return (
    <section aria-label="Ticket page continuation states">
      <TicketPageMore />
      <TicketPageMore loading />
    </section>
  );
}

export function AppEmptyStateDemo() {
  return (
    <section class="empty-state-demo-stack" aria-label="Application empty state variants">
      <AppEmptyState />
      <ProjectRestoreState />
      <AppMessageState title="Project unavailable" message="Open another project to continue working." />
    </section>
  );
}

export function AppErrorDemo() {
  return (
    <section aria-label="Application error feedback">
      <button type="button" data-app-error-demo-show>
        Show error
      </button>
      {appErrorDemoVisible.value && <AppError message="The project could not be opened. Try again." />}
    </section>
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
    'floating-toolbar': { Demo: FloatingToolbarDemo },
    'app-tab': { Demo: AppTabDemo },
    'codex-hooks-notice-banner': { Demo: CodexHooksNoticeBannerDemo },
    'shell-banner': { Demo: ShellBannerDemo },
    'project-setup-warning-banner': { Demo: ProjectSetupWarningBannerDemo },
    'ticket-page-more': { Demo: TicketPageMoreDemo },
    'app-empty-state': { Demo: AppEmptyStateDemo },
    'app-error': { Demo: AppErrorDemo },
  },
  wire: wireAppChromeDemo,
};
