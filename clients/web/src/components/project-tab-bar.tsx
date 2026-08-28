import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import { ChartNoAxesCombined, ChevronsRight, Plus, SquareTerminal } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { ProjectTab, type ProjectTabProps } from './project-tab';
import './project-tab-bar.css';

export interface ProjectTabBarProps {
  tabs: ProjectTabProps[];
  label?: string;
  mode?: ProjectTabBarMode;
  projectsOverflowing?: boolean;
}
export type ProjectTabBarMode = 'project' | 'terminals' | 'stats';

export function visibleProjectTabCount(widths: readonly number[], available: number, gap: number): number {
  let used = 0;
  for (let index = 0; index < widths.length; index += 1) {
    const needed = widths[index] + (index === 0 ? 0 : gap);
    if (used + needed > available + 1) return index;
    used += needed;
  }
  return widths.length;
}

export function syncProjectTabBarOverflow(root: ParentNode): void {
  root.querySelectorAll<HTMLElement>('[data-component="project-tab-bar"]').forEach(bar => {
    const tabs = bar.querySelector<HTMLElement>('.project-tab-bar__tabs');
    const overflow = bar.querySelector<HTMLElement>('[data-project-overflow]');
    if (!tabs || !overflow) return;
    const tabNodes = [...tabs.querySelectorAll<HTMLElement>(':scope > [data-component="project-tab"]')];
    const options = [...overflow.querySelectorAll<HTMLElement>('[data-project-id]')];
    tabNodes.forEach(tab => { tab.dataset.overflowHidden = 'false'; });
    overflow.hidden = true;
    const gap = Number.parseFloat(getComputedStyle(tabs).columnGap) || 0;
    const widths = tabNodes.map(tab => tab.getBoundingClientRect().width);
    const total = widths.reduce((sum, width) => sum + width, 0) + Math.max(0, widths.length - 1) * gap;
    if (total > tabs.clientWidth + 1) {
      overflow.hidden = false;
      const visibleCount = visibleProjectTabCount(widths, tabs.clientWidth, gap);
      widths.forEach((_width, index) => {
        const hidden = index >= visibleCount;
        tabNodes[index].dataset.overflowHidden = String(hidden);
      });
    }
    const hiddenIds = new Set(tabNodes.filter(tab => tab.dataset.overflowHidden === 'true').map(tab => tab.dataset.projectId));
    options.forEach(option => { option.hidden = !hiddenIds.has(option.dataset.projectId); });
  });
}

export function observeProjectTabBarOverflow(root: HTMLElement): () => void {
  const sync = () => requestAnimationFrame(() => syncProjectTabBarOverflow(root));
  const resize = new ResizeObserver(sync);
  const mutation = new MutationObserver(sync);
  resize.observe(root);
  mutation.observe(root, { childList: true, subtree: true });
  sync();
  return () => { resize.disconnect(); mutation.disconnect(); };
}

export function ProjectTabBar({ tabs, label = 'Open projects', mode = 'project', projectsOverflowing = false }: ProjectTabBarProps) {
  return <nav class="project-tab-bar" data-component="project-tab-bar" aria-label={label}>
    <div class="project-tab-bar__modes" role="group" aria-label="Global dashboards">
      <button type="button" data-action="set-shell-mode" data-shell-mode="terminals" aria-label="Terminal dashboard" title="Terminal dashboard" aria-pressed={String(mode === 'terminals')}><LucideIcon icon={SquareTerminal} name="square-terminal" /></button>
      <button type="button" data-action="set-shell-mode" data-shell-mode="stats" aria-label="Cross-project stats" title="Cross-project stats" aria-pressed={String(mode === 'stats')}><LucideIcon icon={ChartNoAxesCombined} name="chart-no-axes-combined" /></button>
    </div>
    <div class="project-tab-bar__tabs" role="tablist" aria-label={label}>{tabs.map(tab => <ProjectTab {...tab} selected={mode === 'project' && tab.selected} />)}</div>
    <div class="project-tab-bar__actions">
      <wa-button appearance="plain" data-action="add-project" aria-label="Add project" title="Add project"><LucideIcon icon={Plus} name="plus" /></wa-button>
      <wa-dropdown placement="bottom-end" data-project-overflow hidden={!projectsOverflowing}>
        <wa-button slot="trigger" appearance="plain" aria-label="More projects" title="More projects"><LucideIcon icon={ChevronsRight} name="chevrons-right" /></wa-button>
        {tabs.map(tab => <wa-dropdown-item data-action="select-project-tab" data-project-id={tab.id} value={tab.id}>{tab.name}<small slot="details">{tab.location}</small></wa-dropdown-item>)}
      </wa-dropdown>
    </div>
  </nav>;
}
