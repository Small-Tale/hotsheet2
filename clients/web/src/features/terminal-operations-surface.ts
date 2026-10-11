import type { Signal } from 'kerfjs';

import { type CheckoutTicketCounts, type TicketRow as WireTicketRow } from '../api';
import { type TerminalOperationsSurfaceProps } from '../components/workspace-composition-surfaces';
import { type Project } from '../interactions/types';
import { ticketCompletionTrend } from '../ticket-completion-trend';

/** Live application bindings createTerminalOperationsSurface reads (HS2-K0VC2C). */
export interface TerminalOperationsSurfaceDependencies {
  partialSourcesByProject: Signal<Record<string, boolean>>;
  projects: Signal<Project[]>;
  projectTicketCounts: (projectId: string) => CheckoutTicketCounts;
  selectedProjectId: Signal<string>;
  ticketRowsByProject: Signal<Record<string, WireTicketRow[]>>;
  tickets: Signal<WireTicketRow[]>;
}

/**
 * Projects the terminal operations dashboard's per-project completion and progress summary.
 * Extracted from the application runtime without behavior changes (HS2-K0VC2C).
 */
export function createTerminalOperationsSurface(dependencies: TerminalOperationsSurfaceDependencies) {
  const { partialSourcesByProject, projects, projectTicketCounts, selectedProjectId, ticketRowsByProject, tickets } =
    dependencies;
  function terminalOperationsSurfaceProps(): TerminalOperationsSurfaceProps {
    return {
      projects: projects.value.map((item) => {
        const rows = item.id === selectedProjectId.value ? tickets.value : (ticketRowsByProject.value[item.id] ?? []),
          counts = projectTicketCounts(item.id),
          trend = counts.completion_trend ?? ticketCompletionTrend(rows);
        return {
          id: item.id,
          name: item.name,
          completedToday: counts.completed_today,
          inProgress: counts.started,
          trend,
          partial: partialSourcesByProject.value[item.id] ?? false,
        };
      }),
    };
  }
  return { terminalOperationsSurfaceProps };
}
