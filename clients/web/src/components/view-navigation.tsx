import './view-navigation.css';

import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import {
  Archive,
  Clock3,
  Ellipsis,
  FileWarning,
  type IconNode,
  Layers3,
  LoaderCircle,
  Pencil,
  Plus,
  Search,
  ShieldAlert,
  Trash2,
} from 'lucide';

import { contextPopupMenuAnchor } from '../context-menu-position';
import { VIEWS_AND_SAVED_VIEWS_ACTIONS } from '../interaction-attrs/views-and-saved-views';

export interface ViewNavigationItem {
  id: string;
  label: string;
  count?: number;
  countLoading?: boolean;
  searchCount?: boolean;
  attention?: boolean;
  icon: 'needs-review' | 'all' | 'backlog' | 'archive' | 'trash' | 'errors' | 'custom';
  manageable?: boolean;
}
export interface ViewNavigationProps {
  items: ViewNavigationItem[];
  selectedId: string;
}
export interface SavedViewContextMenuState {
  id: string;
  label: string;
  x: number;
  y: number;
}
const icons: Record<ViewNavigationItem['icon'], [IconNode, string]> = {
  'needs-review': [ShieldAlert, 'shield-alert'],
  all: [Layers3, 'layers-3'],
  backlog: [Clock3, 'clock-3'],
  archive: [Archive, 'archive'],
  trash: [Trash2, 'trash-2'],
  errors: [FileWarning, 'file-warning'],
  custom: [Search, 'search'],
};
export function ViewNavigation({ items, selectedId }: ViewNavigationProps) {
  return (
    <nav class="view-navigation" data-component="view-navigation" aria-label="Ticket views">
      <ListHeader
        label="Views"
        action="add-view"
        actionLabel="Add view"
        actionIcon={<LucideIcon icon={Plus} name="plus" />}
      />
      <ul>
        {items.map((item) => {
          const [icon, name] = icons[item.icon],
            count =
              item.countLoading || item.count !== undefined ? (
                <small
                  class="view-navigation__count"
                  data-attention={String(Boolean(item.attention))}
                  data-search-count={item.searchCount ? 'true' : undefined}
                  aria-label={
                    item.countLoading
                      ? 'Searching this view'
                      : item.searchCount
                        ? `${item.count} search results`
                        : undefined
                  }
                >
                  {item.countLoading ? (
                    <LucideIcon icon={LoaderCircle} name="loader-circle" />
                  ) : (
                    <>
                      {item.searchCount && <LucideIcon icon={Search} name="search" />}
                      {item.count}
                    </>
                  )}
                </small>
              ) : undefined,
            dropStatus =
              item.id === 'backlog'
                ? 'backlog'
                : item.id === 'archive'
                  ? 'archive'
                  : item.id === 'trash'
                    ? 'deleted'
                    : item.id === 'all'
                      ? 'not_started'
                      : undefined;
          return (
            <li>
              <div
                class="view-navigation__item"
                data-saved-view-id={item.manageable ? item.id : undefined}
                data-saved-view-label={item.manageable ? item.label : undefined}
              >
                <ListItem
                  action="select-view"
                  itemId={item.id}
                  className={item.id === 'errors' ? 'menu-item--errors' : ''}
                  rootAttributes={{ 'data-ticket-drop-status': dropStatus }}
                  selected={item.id === selectedId}
                  icon={<LucideIcon icon={icon} name={name} />}
                  label={item.label}
                  trailing={count}
                />
                {item.manageable && (
                  <span class="view-navigation__meta">
                    <button
                      type="button"
                      class="view-navigation__more"
                      {...VIEWS_AND_SAVED_VIEWS_ACTIONS.openSavedViewMenu.attrs}
                      data-item-id={item.id}
                      data-item-label={item.label}
                      aria-label={`More actions for ${item.label}`}
                      title={`More actions for ${item.label}`}
                      aria-haspopup="menu"
                    >
                      <LucideIcon icon={Ellipsis} name="ellipsis" />
                    </button>
                  </span>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

export function SavedViewContextMenu({ id, label, x, y }: SavedViewContextMenuState) {
  return (
    <div
      class="saved-view-context-menu"
      data-component="saved-view-context-menu"
      role="menu"
      aria-label={`${label} view actions`}
      data-saved-view-id={id}
      {...contextPopupMenuAnchor(x, y)}
    >
      <PopupMenu
        context
        label={`${label} view actions`}
        rootAttributes={{ 'data-context-menu': 'saved-view' }}
        items={[
          {
            label: 'Edit view…',
            action: 'edit-saved-view',
            icon: <LucideIcon icon={Pencil} name="pencil" />,
            attributes: { 'data-item-id': id },
          },
          {
            label: 'Delete view…',
            action: 'delete-saved-view',
            tone: 'danger',
            icon: <LucideIcon icon={Trash2} name="trash-2" />,
            attributes: { 'data-item-id': id },
          },
        ]}
      />
    </div>
  );
}
