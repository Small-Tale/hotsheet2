import './view-navigation.css';

import { Archive, Clock3, FileWarning, type IconNode,Layers3, Pencil, Plus, Search, ShieldAlert, Trash2 } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';

export interface ViewNavigationItem { id: string; label: string; count?: number; attention?: boolean; icon: 'needs-review' | 'all' | 'backlog' | 'archive' | 'errors' | 'custom'; manageable?: boolean }
export interface ViewNavigationProps { items: ViewNavigationItem[]; selectedId: string }
const icons: Record<ViewNavigationItem['icon'], [IconNode, string]> = {
  'needs-review': [ShieldAlert, 'shield-alert'], all: [Layers3, 'layers-3'], backlog: [Clock3, 'clock-3'], archive: [Archive, 'archive'], errors: [FileWarning, 'file-warning'], custom: [Search, 'search'],
};
export function ViewNavigation({ items, selectedId }: ViewNavigationProps) {
  return <nav class="view-navigation" data-component="view-navigation" aria-label="Ticket views">
    <MenuHeader label="Views" action="add-view" actionLabel="Add view" actionIcon={Plus} actionIconName="plus" />
    <ul>{items.map(item => { const [icon, name] = icons[item.icon]; return <li><div class="view-navigation__item"><MenuItem action="select-view" itemId={item.id} className={item.id==='errors'?'menu-item--errors':''} dropStatus={item.id === 'backlog' ? 'backlog' : item.id === 'archive' ? 'archive' : item.id === 'all' ? 'not_started' : undefined} selected={item.id === selectedId} icon={<LucideIcon icon={icon} name={name} />} label={item.label} trailing={item.count !== undefined ? <small class="menu-item__count" data-attention={String(Boolean(item.attention))}>{item.count}</small> : undefined} />{item.manageable&&<span class="view-navigation__actions"><button type="button" data-action="rename-saved-view" data-item-id={item.id} aria-label={`Rename ${item.label}`} title={`Rename ${item.label}`}><LucideIcon icon={Pencil} name="pencil"/></button><button type="button" data-action="delete-saved-view" data-item-id={item.id} aria-label={`Delete ${item.label}`} title={`Delete ${item.label}`}><LucideIcon icon={Trash2} name="trash-2"/></button></span>}</div></li>; })}</ul>
  </nav>;
}
