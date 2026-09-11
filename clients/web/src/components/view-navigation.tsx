import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import './view-navigation.css';

import { Archive, Clock3, Ellipsis, FileWarning, type IconNode, Layers3, Pencil, Plus, Search, ShieldAlert, Trash2 } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';

export interface ViewNavigationItem { id: string; label: string; count?: number; attention?: boolean; icon: 'needs-review' | 'all' | 'backlog' | 'archive' | 'errors' | 'custom'; manageable?: boolean }
export interface ViewNavigationProps { items: ViewNavigationItem[]; selectedId: string }
export interface SavedViewContextMenuState { id:string; label:string; x:number; y:number }
const icons: Record<ViewNavigationItem['icon'], [IconNode, string]> = {
  'needs-review': [ShieldAlert, 'shield-alert'], all: [Layers3, 'layers-3'], backlog: [Clock3, 'clock-3'], archive: [Archive, 'archive'], errors: [FileWarning, 'file-warning'], custom: [Search, 'search'],
};
export function ViewNavigation({ items, selectedId }: ViewNavigationProps) {
  return <nav class="view-navigation" data-component="view-navigation" aria-label="Ticket views">
    <MenuHeader label="Views" action="add-view" actionLabel="Add view" actionIcon={Plus} actionIconName="plus" />
    <ul>{items.map(item => { const [icon, name] = icons[item.icon]; return <li><div class="view-navigation__item" data-saved-view-id={item.manageable?item.id:undefined} data-saved-view-label={item.manageable?item.label:undefined}><MenuItem action="select-view" itemId={item.id} className={item.id==='errors'?'menu-item--errors':''} dropStatus={item.id === 'backlog' ? 'backlog' : item.id === 'archive' ? 'archive' : item.id === 'all' ? 'not_started' : undefined} selected={item.id === selectedId} icon={<LucideIcon icon={icon} name={name} />} label={item.label} /><span class="view-navigation__meta">{item.manageable&&<button type="button" class="view-navigation__more" data-action="open-saved-view-menu" data-item-id={item.id} data-item-label={item.label} aria-label={`More actions for ${item.label}`} title={`More actions for ${item.label}`} aria-haspopup="menu"><LucideIcon icon={Ellipsis} name="ellipsis"/></button>}{item.count !== undefined&&<small class="menu-item__count" data-attention={String(Boolean(item.attention))}>{item.count}</small>}</span></div></li>; })}</ul>
  </nav>;
}

export function SavedViewContextMenu({id,label,x,y}:SavedViewContextMenuState){return <div class="saved-view-context-menu" data-component="saved-view-context-menu" role="menu" aria-label={`${label} view actions`} data-saved-view-id={id} style={`left:${x}px;top:${y}px`}><wa-dropdown-item data-action="edit-saved-view" data-item-id={id}><span slot="icon"><LucideIcon icon={Pencil} name="pencil"/></span>Edit view…</wa-dropdown-item><wa-dropdown-item data-action="delete-saved-view" data-item-id={id} variant="danger"><span slot="icon"><LucideIcon icon={Trash2} name="trash-2"/></span>Delete view…</wa-dropdown-item></div>}
