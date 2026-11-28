import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Eraser } from 'lucide';

import { ciClearAttributes, ciClearItemLabel, type CiClearTarget } from '../ci-presentation';
import { contextMenuAnchor } from '../context-menu-position';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

/** An open Live CI context menu: where, and the shown runs Clear CI Status forgets (HS2-7PSBHQ, HS2-D3JJ49). */
export interface LiveCiMenuState {
  targets: readonly CiClearTarget[];
  x: number;
  y: number;
}

/**
 * The project sidebar Live section's context menu (HS2-7PSBHQ, docs/34-glance-surfaces.md): **Clear CI Status**
 * forgets a shown CI run. A right-click on one run's chip or row offers that run; elsewhere on the section it
 * offers each shown run, named (HS2-D3JJ49). Same pointer-anchored Kerf `PopupMenu` as the other sidebar context
 * menus.
 */
export function LiveCiContextMenu({ targets, x, y }: LiveCiMenuState) {
  const several = targets.length > 1;
  return (
    <PopupMenu
      context
      anchor={contextMenuAnchor(x, y)}
      label="Live Actions"
      rootAttributes={{ 'data-context-menu': 'live-ci' }}
      items={targets.map((target) => ({
        label: ciClearItemLabel(target, several),
        action: COMMANDS_AND_AI_ACTIONS.clearLiveCi.value,
        icon: <LucideIcon icon={Eraser} name="eraser" />,
        attributes: ciClearAttributes(target),
      }))}
    />
  );
}
