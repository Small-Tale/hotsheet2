import { Archive, Clock3, Layers3, Plus, ShieldAlert, type IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './view-navigation.css';

export interface ViewNavigationItem { id: string; label: string; count?: number; attention?: boolean; icon: 'needs-review' | 'all' | 'backlog' | 'archive' }
export interface ViewNavigationProps { items: ViewNavigationItem[]; selectedId: string }
const icons: Record<ViewNavigationItem['icon'], [IconNode, string]> = {
  'needs-review': [ShieldAlert, 'shield-alert'], all: [Layers3, 'layers-3'], backlog: [Clock3, 'clock-3'], archive: [Archive, 'archive'],
};
export function ViewNavigation({ items, selectedId }: ViewNavigationProps) {
  return <nav class="view-navigation" data-component="view-navigation" aria-label="Ticket views">
    <header><h2>Views</h2><button type="button" data-action="add-view" aria-label="Add view"><LucideIcon icon={Plus} name="plus" /></button></header>
    <ul>{items.map(item => { const [icon, name] = icons[item.icon]; return <li><button type="button" data-action="select-view" data-view-id={item.id} aria-current={item.id === selectedId ? 'page' : undefined}><LucideIcon icon={icon} name={name} /><span>{item.label}</span>{item.count !== undefined && <small data-attention={String(Boolean(item.attention))}>{item.count}</small>}</button></li>; })}</ul>
  </nav>;
}
