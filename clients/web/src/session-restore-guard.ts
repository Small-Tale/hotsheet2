import type { InspectorTab } from './components/ticket-inspector';
import type { TicketView } from './ticket-views';

/** The section tabs and ticket view a parked project session holds. */
export interface SessionSectionTabs {
  inspector: InspectorTab;
  reader: InspectorTab;
  view: TicketView;
}

/**
 * Whether a parked-session restore may replace the ticket selection (HS2-VDJJEY). It may while the
 * selection is still the array the activation or an earlier restore applied; any other array is a
 * selection the user made since. With no record, nothing has been applied yet.
 *
 * One activation can restore more than once: an early restore when the first tickets arrive, then a
 * full one, and a project activated twice while it opens runs a restore per activation. The record
 * must therefore outlive each restore; a one-shot record let the second restore replace a ticket the
 * user had selected between them with the parked selection (HS2-CN0CS7).
 */
export function selectionRestorable(applied: readonly string[] | undefined, current: readonly string[]): boolean {
  return applied === undefined || current === applied;
}

/**
 * The section tabs and ticket view a restore shows, and the record the next restore compares
 * against (HS2-KBM5QK, HS2-123CV9, HS2-CN0CS7). Each part the user has not changed since it was
 * last applied takes the parked value; a part the user changed keeps the user's choice, and its
 * record stays the earlier applied value so a later restore also leaves it alone.
 */
export function restoreSectionTabs(
  applied: SessionSectionTabs | undefined,
  current: SessionSectionTabs,
  parked: SessionSectionTabs,
): { show: SessionSectionTabs; applied: SessionSectionTabs } {
  const part = <Key extends keyof SessionSectionTabs>(key: Key) =>
    !applied || current[key] === applied[key]
      ? { show: parked[key], applied: parked[key] }
      : { show: current[key], applied: applied[key] };
  const inspector = part('inspector'),
    reader = part('reader'),
    view = part('view');
  return {
    show: { inspector: inspector.show, reader: reader.show, view: view.show },
    applied: { inspector: inspector.applied, reader: reader.applied, view: view.applied },
  };
}
