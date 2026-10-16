import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { OctagonAlert, TriangleAlert } from 'lucide';

import type { RepositorySizeLevel, RepositorySizeReport } from '../api';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { formatBytes, limitSubject } from './repository-size';
import { ShellBanner } from './shell-banner';

const LEVEL_RANK: Record<RepositorySizeLevel, number> = { ok: 0, warning: 1, critical: 2 };

/**
 * Whether a project's repository size report warrants the shell banner: some repository is
 * over a GitHub limit, and the user has not dismissed this level (or a higher one) during this
 * open. A report that rises past a dismissed level shows the banner again (HS2-P1GYWH).
 */
export function showsRepositorySizeBanner(
  report: RepositorySizeReport | undefined,
  dismissed: RepositorySizeLevel | undefined,
): report is RepositorySizeReport {
  if (!report || report.level === 'ok') return false;
  return dismissed === undefined || LEVEL_RANK[report.level] > LEVEL_RANK[dismissed];
}

/**
 * A persistent project-level strip while any of the project's repositories is over a GitHub
 * size limit (HS2-P1GYWH). The server computes the level; this only presents it, links to the
 * project statistics' Repository Size section, and can be dismissed for the current level.
 * Renders nothing while every repository is within limits or this level was dismissed.
 */
export function RepositorySizeBanner({
  report,
  dismissed,
  projectId,
}: {
  report: RepositorySizeReport | undefined;
  dismissed?: RepositorySizeLevel;
  projectId: string;
}) {
  if (!showsRepositorySizeBanner(report, dismissed)) return;
  const critical = report.level === 'critical',
    { subject, verb } = limitSubject(report),
    limit = formatBytes(critical ? report.thresholds.critical_bytes : report.thresholds.warning_bytes);
  return (
    <ShellBanner
      component="repository-size-banner"
      tone="warning"
      title={`${subject} ${verb} over GitHub's ${limit} ${critical ? 'hard' : 'soft'} limit`}
      detail={
        critical
          ? 'GitHub rejects pushes to repositories this large. Remove large attachments from its history before the next push.'
          : `GitHub rejects repositories over ${formatBytes(report.thresholds.critical_bytes)}. Large ticket attachments are the usual cause.`
      }
      actionPlacement="below"
      icon={
        critical ? (
          <LucideIcon icon={OctagonAlert} name="octagon-alert" />
        ) : (
          <LucideIcon icon={TriangleAlert} name="triangle-alert" />
        )
      }
      action={
        <Row gap="xs">
          <button
            type="button"
            {...PROJECT_LIFECYCLE_ACTIONS.dismissRepositorySizeWarning.attrs}
            data-level={report.level}
          >
            Dismiss
          </button>
          <button type="button" {...NAVIGATION_AND_TABS_ACTIONS.openProjectStats.attrs} data-project-id={projectId}>
            View Repository Size
          </button>
        </Row>
      }
    />
  );
}
