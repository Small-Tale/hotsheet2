import { ProgressRing } from '@kerfjs/ui/progress-ring';

import { ShellBanner } from './shell-banner';

export interface TicketGroupOperation {
  projectId: string;
  /** What is running, as a sentence title: "Merging Launch readiness into Design system". */
  title: string;
}

/**
 * A non-blocking banner while a Ticket groups merge or delete runs (HS2-GXKX1Q, Design v6): the page
 * stays usable and the person can leave it. The server rewrites every member in one write per source
 * and reports no progress while it runs, so the ring is indeterminate until a job API reports counts.
 */
export function TicketGroupOperationBanner({ operation }: { operation?: TicketGroupOperation }) {
  if (!operation) return;
  return (
    <ShellBanner
      component="ticket-group-operation-banner"
      tone="info"
      title={`${operation.title}…`}
      detail="You can keep working; the groups list updates when it finishes."
      icon={<ProgressRing size="small" label={operation.title} />}
    />
  );
}
