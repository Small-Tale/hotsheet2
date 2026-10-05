import './repository-summary.css';

import { Badge } from '@kerfjs/ui/badge';
import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { ArrowDown, ArrowUp, CircleAlert, GitBranch } from 'lucide';

/** The summary's icons render below Kerf's 18px ListItem icon default, through LucideIcon's own size (HS2-QM0C3T). */
const REPOSITORY_SUMMARY_ICON_SIZE = 14.4;

export interface RepositorySummaryProps {
  branch: string;
  unpushed: number;
  behind?: number;
  uncommitted: number;
  conflicted?: number;
  error?: boolean;
}
export function RepositorySummary({
  branch,
  unpushed,
  behind = 0,
  uncommitted,
  conflicted = 0,
  error = false,
}: RepositorySummaryProps) {
  const accessible = error
    ? `Repository status unavailable for ${branch}`
    : `Repository status for ${branch}: ${unpushed} ahead, ${behind} behind, ${uncommitted} uncommitted, ${conflicted} conflicted`;
  return (
    <div
      class="repository-summary"
      data-component="repository-summary"
      data-state={
        error
          ? 'error'
          : conflicted
            ? 'conflicted'
            : uncommitted
              ? 'dirty'
              : behind
                ? 'behind'
                : unpushed
                  ? 'ahead'
                  : 'clean'
      }
    >
      <ListItem
        action="open-repository-status"
        accessibleLabel={accessible}
        icon={
          <LucideIcon
            icon={error || conflicted ? CircleAlert : GitBranch}
            name={error || conflicted ? 'circle-alert' : 'git-branch'}
            size={REPOSITORY_SUMMARY_ICON_SIZE}
          />
        }
        label={<span class="repository-summary__branch-name">{branch}</span>}
        trailing={
          <Row gap="xs" vAlign="middle">
            {error ? (
              <Text variant="span" tone="danger" size="compact">
                Unavailable
              </Text>
            ) : (
              <>
                <Row gap="2xs" vAlign="middle">
                  <LucideIcon icon={ArrowUp} name="arrow-up" size={REPOSITORY_SUMMARY_ICON_SIZE} />
                  <Text
                    variant="span"
                    size="compact"
                    color={foregroundColorVar('--wa-color-text-link')}
                    title={`${unpushed} unpushed commits`}
                  >
                    {unpushed}
                  </Text>
                </Row>
                {behind > 0 && (
                  <Row gap="2xs" vAlign="middle">
                    <LucideIcon icon={ArrowDown} name="arrow-down" size={REPOSITORY_SUMMARY_ICON_SIZE} />
                    <Text
                      variant="span"
                      size="compact"
                      color={foregroundColorVar('--wa-color-text-link')}
                      title={`${behind} commits behind`}
                    >
                      {behind}
                    </Text>
                  </Row>
                )}
                <Badge tone="neutral" appearance="quiet" size="compact" label={`${uncommitted} uncommitted changes`}>
                  {uncommitted}
                </Badge>
              </>
            )}
          </Row>
        }
      />
    </div>
  );
}
