import './view-navigation.css';

import { Badge } from '@kerfjs/ui/badge';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import {
  Archive,
  Clock3,
  Ellipsis,
  FileWarning,
  type IconNode,
  Layers3,
  Pencil,
  Plus,
  Search,
  ShieldAlert,
  Trash2,
} from 'lucide';

import { contextPopupMenuAnchor } from '../context-menu-position';

export interface ViewNavigationItem {
  id: string;
  label: string;
  count?: number;
  countPartial?: boolean;
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
      <List gap="2xs">
        <ListHeader
          label="Views"
          action="add-view"
          actionLabel="Add view"
          actionIcon={<LucideIcon icon={Plus} name="plus" />}
        />
        <List gap="none" className="view-navigation__list">
          {items.map((item) => {
            const [icon, name] = icons[item.icon],
              count =
                item.countLoading || item.count !== undefined ? (
                  <span
                    class="view-navigation__count"
                    data-attention={String(Boolean(item.attention))}
                    data-search-count={item.searchCount ? 'true' : undefined}
                  >
                    {item.countLoading ? (
                      <LoadingSpinner size={9.92} label="Searching this view" />
                    ) : (
                      <>
                        {item.searchCount ? <LucideIcon size={9.92} icon={Search} name="search" /> : null}
                        <Badge
                          appearance="quiet"
                          size="compact"
                          tone={item.attention ? 'danger' : 'neutral'}
                          label={
                            item.countPartial
                              ? `At least ${item.count} results; a ticket source is unavailable`
                              : item.searchCount
                                ? `${item.count} search results`
                                : undefined
                          }
                        >
                          {item.countPartial ? `≥${item.count}` : String(item.count)}
                        </Badge>
                      </>
                    )}
                  </span>
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
              <div
                class="view-navigation__item"
                data-view-tone={item.id === 'errors' ? 'danger' : undefined}
                data-saved-view-id={item.manageable ? item.id : undefined}
                data-saved-view-label={item.manageable ? item.label : undefined}
              >
                {item.manageable ? (
                  <ListActionRow
                    action="select-view"
                    itemId={item.id}
                    selected={item.id === selectedId}
                    icon={<LucideIcon icon={icon} name={name} />}
                    label={item.label}
                    status={count}
                    trailingAction="open-saved-view-menu"
                    trailingActionLabel={`More actions for ${item.label}`}
                    trailingActionTitle={`More actions for ${item.label}`}
                    trailingActionIcon={<LucideIcon size={14.4} icon={Ellipsis} name="ellipsis" />}
                    trailingActionAttributes={{
                      'data-item-label': item.label,
                      'aria-haspopup': 'menu',
                    }}
                  />
                ) : (
                  <ListItem
                    action="select-view"
                    itemId={item.id}
                    rootAttributes={{ 'data-ticket-drop-status': dropStatus }}
                    selected={item.id === selectedId}
                    icon={<LucideIcon icon={icon} name={name} />}
                    label={item.label}
                    trailing={count}
                  />
                )}
              </div>
            );
          })}
        </List>
      </List>
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
