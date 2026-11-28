import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Eraser } from 'lucide';

import type { CiClearTarget } from '../ci-presentation';
import { contextMenuAnchor } from '../context-menu-position';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

/** An open Live CI context menu: where, and the stored run Clear CI Status forgets (HS2-7PSBHQ). */
export interface LiveCiMenuState extends CiClearTarget {
  x: number;
  y: number;
}

/**
 * The project sidebar Live section's context menu (HS2-7PSBHQ, docs/34-glance-surfaces.md): while the `main`
 * CI run is shown as unknown, **Clear CI Status** forgets the stored report whose end nobody reported. Same
 * pointer-anchored Kerf `PopupMenu` as the other sidebar context menus.
 */
export function LiveCiContextMenu({ checkout, reportedAtMs, x, y }: LiveCiMenuState) {
  return (
    <PopupMenu
      context
      anchor={contextMenuAnchor(x, y)}
      label="Live Actions"
      rootAttributes={{ 'data-context-menu': 'live-ci' }}
      items={[
        {
          label: 'Clear CI Status',
          action: COMMANDS_AND_AI_ACTIONS.clearLiveCi.value,
          icon: <LucideIcon icon={Eraser} name="eraser" />,
          attributes: { 'data-ci-checkout': checkout, 'data-ci-reported-at-ms': String(reportedAtMs) },
        },
      ]}
    />
  );
}
