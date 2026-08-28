import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import { ArrowRight, CircleX, Trash2, X, type IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './project-tab-context-menu.css';

const actions: ReadonlyArray<{ id: string; label: string; icon: IconNode; iconName: string; danger?: boolean }> = [
  { id: 'close', label: 'Close Tab', icon: X, iconName: 'x' },
  { id: 'close-others', label: 'Close Other Tabs', icon: CircleX, iconName: 'circle-x' },
  { id: 'close-right', label: 'Close Tabs to the Right', icon: ArrowRight, iconName: 'arrow-right' },
  { id: 'close-all', label: 'Close All Tabs', icon: Trash2, iconName: 'trash-2', danger: true },
];

export function ProjectTabContextMenu({ projectId, x, y }: { projectId: string; x: number; y: number }) {
  return <div class="project-tab-context-menu" role="menu" aria-label="Project tab actions" style={`left:${x}px;top:${y}px`} data-project-id={projectId}>
    {actions.map((item, index) => <>{index === 3 && <wa-divider></wa-divider>}<wa-dropdown-item data-action="project-tab-context-action" data-tab-action={item.id} data-project-id={projectId} variant={item.danger ? 'danger' : undefined}><span slot="icon"><LucideIcon icon={item.icon} name={item.iconName} /></span>{item.label}</wa-dropdown-item></>)}
  </div>;
}
