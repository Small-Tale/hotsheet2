import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { ArrowLeft, ArrowRight, CircleX, Hand, HandHelping, type IconNode, Pause, Pencil, Trash2, X } from 'lucide';

import type { TerminalOrchestrator } from '../api';
import { contextMenuAnchor } from '../context-menu-position';

export type AppTabKind = 'project' | 'terminal' | 'ai-chat';
/** An open tab context menu: where, which tab, and what the tab's terminal runs. */
export interface AppTabMenuState {
  x: number;
  y: number;
  kind: AppTabKind;
  id: string;
  direction: 'left' | 'right';
  /** HS2-6MNMVB: the orchestrator session in a terminal tab, when one started it here. */
  orchestrator?: Pick<TerminalOrchestrator, 'owner'>;
}
export function AppTabContextMenu({
  kind,
  id,
  x,
  y,
  direction = 'right',
  orchestrator,
}: {
  kind: AppTabKind;
  id: string;
  x: number;
  y: number;
  direction?: 'left' | 'right';
  orchestrator?: Pick<TerminalOrchestrator, 'owner'>;
}) {
  const directional =
    direction === 'left'
      ? { id: 'close-left', label: 'Close Tabs to the Left', icon: ArrowLeft, iconName: 'arrow-left' }
      : { id: 'close-right', label: 'Close Tabs to the Right', icon: ArrowRight, iconName: 'arrow-right' };
  const actions: ReadonlyArray<{ id: string; label: string; icon: IconNode; iconName: string; danger?: boolean }> = [
    ...(kind === 'terminal' ? [{ id: 'rename', label: 'Rename…', icon: Pencil, iconName: 'pencil' }] : []),
    // HS2-6MNMVB: a person takes an orchestrator-started session over at this machine; once it is
    // theirs the orchestrator leaves it alone, and these are no longer offered.
    ...(kind === 'terminal' && orchestrator?.owner === 'orchestrator'
      ? [
          { id: 'orchestrator-takeover', label: 'Take Over from Orchestrator', icon: Hand, iconName: 'hand' },
          { id: 'orchestrator-pause', label: 'Take Over and Pause', icon: Pause, iconName: 'pause' },
        ]
      : []),
    // HS2-KR28FS: a session a person took over or paused can go back to its orchestrator.
    ...(kind === 'terminal' && orchestrator?.owner === 'human'
      ? [
          {
            id: 'orchestrator-hand-back',
            label: 'Hand Back to Orchestrator',
            icon: HandHelping,
            iconName: 'hand-helping',
          },
        ]
      : []),
    { id: 'close', label: 'Close Tab', icon: X, iconName: 'x' },
    { id: 'close-others', label: 'Close Other Tabs', icon: CircleX, iconName: 'circle-x' },
    directional,
    { id: 'close-all', label: 'Close All Tabs', icon: Trash2, iconName: 'trash-2', danger: true },
  ];
  const { legacyId, label, action } = (() => {
    switch (kind) {
      case 'project':
        return {
          legacyId: { 'data-project-id': id },
          label: 'Project',
          action: 'project-tab-context-action',
        };
      case 'terminal':
        return {
          legacyId: { 'data-terminal-id': id },
          label: 'Terminal',
          action: 'terminal-tab-context-action',
        };
      case 'ai-chat':
        return {
          legacyId: { 'data-chat-id': id },
          label: 'AI Chat',
          action: 'terminal-tab-context-action',
        };
      default:
        return kind satisfies never;
    }
  })();
  return (
    <PopupMenu
      context
      anchor={contextMenuAnchor(x, y)}
      label={`${label} Tab Actions`}
      rootAttributes={{ 'data-context-menu': 'app-tab', 'data-tab-kind': kind, 'data-tab-id': id, ...legacyId }}
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
