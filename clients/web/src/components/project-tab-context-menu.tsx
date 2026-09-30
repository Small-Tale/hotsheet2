import './project-tab-context-menu.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { ArrowLeft, ArrowRight, CircleX, type IconNode, Pencil, Trash2, X } from 'lucide';

import { contextPopupMenuAnchor } from '../context-menu-position';

export type AppTabKind = 'project' | 'terminal' | 'ai-chat';
export function AppTabContextMenu({
  kind,
  id,
  x,
  y,
  direction = 'right',
}: {
  kind: AppTabKind;
  id: string;
  x: number;
  y: number;
  direction?: 'left' | 'right';
}) {
  const directional =
    direction === 'left'
      ? { id: 'close-left', label: 'Close Tabs to the Left', icon: ArrowLeft, iconName: 'arrow-left' }
      : { id: 'close-right', label: 'Close Tabs to the Right', icon: ArrowRight, iconName: 'arrow-right' };
  const actions: ReadonlyArray<{ id: string; label: string; icon: IconNode; iconName: string; danger?: boolean }> = [
    ...(kind === 'terminal' ? [{ id: 'rename', label: 'Rename…', icon: Pencil, iconName: 'pencil' }] : []),
    { id: 'close', label: 'Close Tab', icon: X, iconName: 'x' },
    { id: 'close-others', label: 'Close Other Tabs', icon: CircleX, iconName: 'circle-x' },
    directional,
    { id: 'close-all', label: 'Close All Tabs', icon: Trash2, iconName: 'trash-2', danger: true },
  ];
  const legacyId =
      kind === 'project'
        ? { 'data-project-id': id }
        : kind === 'terminal'
          ? { 'data-terminal-id': id }
          : { 'data-chat-id': id },
    label = kind === 'project' ? 'Project' : kind === 'terminal' ? 'Terminal' : 'AI chat';
  const action = kind === 'project' ? 'project-tab-context-action' : 'terminal-tab-context-action';
  return (
    <div
      class="project-tab-context-menu app-tab-context-menu"
      role="menu"
      aria-label={`${label} tab actions`}
      {...contextPopupMenuAnchor(x, y)}
      data-tab-kind={kind}
      data-tab-id={id}
      {...legacyId}
    >
      <PopupMenu
        context
        label={`${label} tab actions`}
        rootAttributes={{ 'data-context-menu': 'app-tab' }}
        items={actions.flatMap((item): PopupMenuEntry[] => [
          ...(item.id === 'close-all' ? [{ kind: 'divider' } as const] : []),
          {
            label: item.label,
            action,
            tone: item.danger ? 'danger' : 'default',
            icon: <LucideIcon icon={item.icon} name={item.iconName} />,
            attributes: { 'data-tab-action': item.id, 'data-tab-kind': kind, 'data-tab-id': id, ...legacyId },
          },
        ])}
      />
    </div>
  );
}

export function ProjectTabContextMenu({
  projectId,
  x,
  y,
  direction = 'right',
}: {
  projectId: string;
  x: number;
  y: number;
  direction?: 'left' | 'right';
}) {
  return <AppTabContextMenu kind="project" id={projectId} x={x} y={y} direction={direction} />;
}
