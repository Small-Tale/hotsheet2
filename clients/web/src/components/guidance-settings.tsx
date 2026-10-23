import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { rem } from '@kerfjs/ui/css-values';
import { Disclosure } from '@kerfjs/ui/disclosure';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import type {
  GuidanceInterviewReview,
  GuidanceOverview,
  GuidanceRecommendationReport,
  GuidanceRulePreview,
  GuidanceSourceReview,
  GuidanceSourceUpdate,
  RulesyncUpdateReport,
} from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

export interface GuidanceSettingsProps {
  overview?: GuidanceOverview;
  updates?: readonly GuidanceSourceUpdate[];
  updatesCheckedAt?: number;
  updatesLoading?: boolean;
  updatesError?: string;
  review?: GuidanceSourceReview;
  reviewLoading?: boolean;
  reviewError?: string;
  recommendations?: GuidanceRecommendationReport;
  recommendationsLoading?: boolean;
  recommendationsError?: string;
  recommendationDecisionBusy?: string;
  interview?: GuidanceInterviewReview;
  interviewLoading?: boolean;
  interviewError?: string;
  interviewBusy?: boolean;
  interviewAnswerDraft?: string;
  rulesyncUpdateReport?: RulesyncUpdateReport | null;
  loading: boolean;
  message: string;
  query: string;
  repository: string;
  sourceRef: string;
  sourceId: string;
  ownerFilter: string;
  tagFilter: string;
  rulePreview?: {
    source: string;
    rule: string;
    loading: boolean;
    error: string;
    result?: GuidanceRulePreview;
  };
  bundledGuidanceBlocked?: readonly {
    path: string;
    section: string;
    installedVersion: number;
    bundledVersion: number;
  }[];
}

/** Project Settings → AI tools: discover and apply pinned guidance (HS2-2RD1M1). */
export function GuidanceSettings(props: GuidanceSettingsProps) {
  const { overview, loading, message, query, repository, sourceRef, sourceId, ownerFilter, tagFilter } = props;
  const rules = overview?.sources.flatMap((source) => source.rules) ?? [];
  const owners = [
    ...new Set(rules.map((rule) => rule.owner).filter((owner): owner is string => Boolean(owner))),
  ].sort();
  const tags = [...new Set(rules.flatMap((rule) => rule.tags))].sort();
  const registeredSource = overview?.sources.find((source) => source.id === sourceId);
  const removedSelected =
    props.review?.changes.some((change) => change.change === 'removed' && change.selected) ?? false;
  return (
    <section data-component="guidance-settings" aria-label="Project Guidance" aria-busy={String(loading)}>
      <List gap="l">
        <Select
          name="guidance-owner"
          label="Instruction Writer"
          value={overview?.owner ?? 'builtin'}
          choices={[
            { value: 'builtin', label: 'Hot Sheet Built-in' },
            { value: 'rulesync', label: 'Rulesync' },
          ]}
        />
        {overview?.owner === 'rulesync' && (
          <List gap="xs">
            <Text tone="quiet">
              Rulesync regenerates <code>AGENTS.md</code> from its rule inputs. Hot Sheet saves unmarked local guidance
              in <code>.rulesync/rules/project-local.md</code> when you switch writers. Review that rule before
              applying.
            </Text>
            {(overview.local_rules ?? []).length > 0 && (
              <section aria-label="Project-Local Rulesync Rules">
                <Text variant="h3">Project-local rules</Text>
                {(overview.local_rules ?? []).map((name) => (
                  <Text>
                    {name} · .rulesync/rules/{name}.md
                  </Text>
                ))}
              </section>
            )}
          </List>
        )}
        {!!props.bundledGuidanceBlocked?.length && (
          <StateBanner
            title="Bundled Guidance Needs Review"
            detail={`${props.bundledGuidanceBlocked.map((block) => `${block.path}: ${block.section} (local v${block.installedVersion}, bundled v${block.bundledVersion}); compare with plugins/shared/${block.section}.md`).join('; ')}. Hot Sheet preserved the local text. Run hotsheet-cli -C <ticket-store> setup --diagnose --project <code-project> --json, reconcile the named sections, then run setup --refresh.`}
            tone="warning"
            urgency="status"
            copyLayout="stacked"
          />
        )}
        {overview?.pending_consolidation_review && (
          <StateBanner
            title="Rulesync Guidance Review Pending"
            detail="Hot Sheet still needs to create a follow-up ticket for this guidance change. In Project Settings → Ticket sources, link or select a writable default source. Hot Sheet will retry automatically. To retry headlessly, run hotsheet-cli guidance --project <code-project> check-rulesync-updates."
            tone="warning"
            urgency="status"
            copyLayout="stacked"
          />
        )}
        <section aria-label="Guidance Recommendations" data-guidance-recommendations="">
          <List gap="xs">
            <Text variant="h3">Recommendations</Text>
            <Text tone="quiet" size="compact">
              Suggestions use local project signals and the exact versions of registered pinned sources. Review each
              choice before applying guidance.
            </Text>
            <Button
              action={COMMANDS_AND_AI_ACTIONS.recheckGuidanceRecommendations.value}
              disabled={loading || props.recommendationsLoading || !!props.recommendationDecisionBusy}
            >
              {props.recommendationsLoading ? 'Checking…' : 'Recheck Recommendations'}
            </Button>
            {props.recommendationsLoading && !props.recommendations && (
              <Text tone="quiet" size="compact">
                Checking local project signals…
              </Text>
            )}
            {props.recommendationsError ? (
              <StateBanner
                title="Recommendation Status Unknown"
                detail={`${props.recommendationsError} Previous suggestions may be stale; no selection was changed.`}
                tone="warning"
                urgency="status"
                copyLayout="stacked"
              />
            ) : null}
            {props.recommendations?.changes.length ? (
              <StateBanner
                title="New Guidance Recommendations Are Available"
                detail={`${props.recommendations.changes.length} new, changed, or withdrawn suggestion(s) since review. Review the changes below; nothing is applied automatically.`}
                tone="info"
                urgency="status"
                copyLayout="stacked"
              />
            ) : null}
            {props.recommendations?.source_registration_suggested && (
              <Text tone="quiet">Register a pinned source to receive project-specific suggestions.</Text>
            )}
            {props.recommendations?.first_accept_replaces_bundled_default &&
              props.recommendations.candidates.some((candidate) => !candidate.selected) && (
                <Text tone="quiet" size="compact">
                  The first accepted selection replaces the bundled default when you later apply guidance.
                </Text>
              )}
            {props.recommendations?.changes
              .filter((change) => change.change === 'withdrawn')
              .map((change) => (
                <Text tone="quiet" size="compact">
                  Withdrawn: {change.source} · {change.kind} {change.name}. Installed guidance remains unchanged.
                </Text>
              ))}
            {props.recommendations?.candidates.map((candidate) => {
              const change = props.recommendations?.changes.find(
                (item) =>
                  item.source === candidate.source && item.kind === candidate.kind && item.name === candidate.name,
              );
              const decisionKey = `${candidate.source}:${candidate.kind}:${candidate.name}`;
              const busy = props.recommendationDecisionBusy === decisionKey;
              return (
                <section
                  aria-label={`${candidate.source} ${candidate.kind} ${candidate.name}`}
                  data-recommendation={decisionKey}
                >
                  <List gap="xs">
                    <Text>{candidate.description}</Text>
                    <Text tone="quiet" size="compact">
                      {candidate.source} · {candidate.kind} {candidate.name}
                      {candidate.kind === 'rule' ? ` · available v${candidate.version}` : ''} · pinned{' '}
                      {candidate.pinned_commit.slice(0, 12)} · {change?.change ?? candidate.state}
                      {candidate.installed_version !== null ? ` · installed v${candidate.installed_version}` : ''}
                    </Text>
                    <Text tone="quiet" size="compact">
                      Evidence: {candidate.evidence.join(', ')}
                    </Text>
                    {!candidate.selected && (
                      <div>
                        <Button
                          action={COMMANDS_AND_AI_ACTIONS.acceptGuidanceRecommendation.value}
                          rootAttributes={{
                            'data-recommendation-source': candidate.source,
                            'data-recommendation-kind': candidate.kind,
                            'data-recommendation-name': candidate.name,
                            'data-recommendation-fingerprint': candidate.fingerprint,
                          }}
                          disabled={loading || busy || !!props.recommendationDecisionBusy}
                        >
                          {busy ? 'Reviewing…' : 'Accept Selection'}
                        </Button>{' '}
                        <Button
                          action={COMMANDS_AND_AI_ACTIONS.dismissGuidanceRecommendation.value}
                          rootAttributes={{
                            'data-recommendation-source': candidate.source,
                            'data-recommendation-kind': candidate.kind,
                            'data-recommendation-name': candidate.name,
                            'data-recommendation-fingerprint': candidate.fingerprint,
                          }}
                          disabled={
                            loading || busy || !!props.recommendationDecisionBusy || candidate.state === 'dismissed'
                          }
                        >
                          Dismiss
                        </Button>
                      </div>
                    )}
                  </List>
                </section>
              );
            })}
            {props.recommendations &&
              !props.recommendations.candidates.length &&
              !props.recommendations.source_registration_suggested && (
                <Text tone="quiet">No guidance suggestions match the current project signals.</Text>
              )}
          </List>
        </section>
        {!props.interview && props.interviewError ? (
          <StateBanner
            title="Interview Status Unknown"
            detail={props.interviewError}
            tone="warning"
            urgency="status"
            copyLayout="stacked"
          />
        ) : null}
        {!props.interview && props.interviewLoading ? (
          <Text tone="quiet" size="compact">
            Checking optional interview status…
          </Text>
        ) : null}
        {props.interview?.preview.questions.length ? (
          <section aria-label="Optional Guidance Interview" data-guidance-interview="">
            <List gap="xs">
              <Text variant="h3">Optional AI interview</Text>
              <Text tone="quiet" size="compact">
                Local evidence leaves some choices open. Review the exact context below before choosing to share it with
                an AI tool. Hot Sheet will not send it or change guidance for you.
              </Text>
              {props.interview.preview.questions.map((question) => (
                <Text size="compact">
                  {question.prompt} ({question.choices.join(', ')})
                </Text>
              ))}
              <TextField
                label="Exact Context and Prompt to Share"
                multiline
                rows={8}
                readOnly
                value={props.interview.preview.outbound_prompt ?? ''}
              />
              {props.interview.state === 'not_started' ? (
                <Button
                  action={COMMANDS_AND_AI_ACTIONS.openGuidanceInterview.value}
                  disabled={props.interviewBusy || loading}
                >
                  I Choose to Share This Context
                </Button>
              ) : (
                <>
                  <Button action={COMMANDS_AND_AI_ACTIONS.copyGuidanceInterview.value} disabled={props.interviewBusy}>
                    Copy Prompt for AI
                  </Button>
                  <Text tone="quiet" size="compact">
                    Paste the prompt into an AI tool you choose. Continue that conversation there, then paste its final
                    advisory JSON here. Advice cannot select or apply guidance.
                  </Text>
                  {props.interview.state === 'awaiting_answer' ? (
                    <>
                      <TextField
                        name="guidance-interview-answer"
                        label="AI Advisory JSON"
                        multiline
                        rows={6}
                        value={props.interviewAnswerDraft ?? ''}
                      />
                      <Button
                        action={COMMANDS_AND_AI_ACTIONS.submitGuidanceInterview.value}
                        disabled={props.interviewBusy || !(props.interviewAnswerDraft ?? '').trim()}
                      >
                        Review Advisory Answer
                      </Button>
                    </>
                  ) : null}
                </>
              )}
              {props.interview.answer?.advice.map((advice) => {
                const candidate = props.interview?.preview.context.candidates.find(
                  (item) => item.id === advice.candidate_id,
                );
                return (
                  <Text size="compact">
                    {candidate ? `${candidate.source} · ${candidate.kind} ${candidate.name}` : advice.candidate_id}:{' '}
                    {advice.stance} — {advice.explanation}
                  </Text>
                );
              })}
              {props.interviewError ? (
                <StateBanner
                  title="Interview Status Unknown"
                  detail={props.interviewError}
                  tone="warning"
                  urgency="status"
                  copyLayout="stacked"
                />
              ) : null}
              {props.interviewLoading && (
                <Text tone="quiet" size="compact">
                  Checking interview status…
                </Text>
              )}
            </List>
          </section>
        ) : null}
        {props.rulesyncUpdateReport && (
          <section data-rulesync-update-status="" aria-label="Standalone Rulesync Lock Status">
            <List gap="xs">
              <StateBanner
                title={
                  {
                    current: 'Rulesync Lock Is Current',
                    outdated: 'Rulesync Lock Has Updates',
                    unlocked: 'Rulesync Sources Need A Lock',
                    unknown: 'Rulesync Lock Status Is Unknown',
                  }[props.rulesyncUpdateReport.state]
                }
                detail={
                  props.rulesyncUpdateReport.state === 'current'
                    ? `Rulesync ${props.rulesyncUpdateReport.executable_version} checked ${props.rulesyncUpdateReport.sources.length} standalone source(s) against rulesync.lock.`
                    : `Rulesync ${props.rulesyncUpdateReport.executable_version ?? 'version unknown'}: ${props.rulesyncUpdateReport.reason ? `${props.rulesyncUpdateReport.reason}.` : `${props.rulesyncUpdateReport.state} source locks.`} Review rulesync.lock before running install --update. Then inspect the changed lock and rulesync generate --dry-run diff before generate. Headless check: hotsheet-cli guidance --project <code-project> check-rulesync-updates.`
                }
                tone={props.rulesyncUpdateReport.state === 'current' ? 'success' : 'warning'}
                urgency="status"
                copyLayout="stacked"
              />
              {props.rulesyncUpdateReport.sources.map((source) => (
                <Text tone="quiet" size="compact">
                  {source.source}: {source.state}
                  {source.locked_ref && ` · locked ${source.locked_ref.slice(0, 12)}`}
                  {source.latest_ref && ` · observed ${source.latest_ref.slice(0, 12)}`}
                </Text>
              ))}
            </List>
          </section>
        )}
        {!!overview?.sources.length && (
          <section aria-label="Pinned Source Updates" data-guidance-updates="">
            <List gap="xs">
              <Text variant="h3">Pinned source updates</Text>
              <Text tone="quiet" size="compact">
                {props.updatesCheckedAt
                  ? `Last checked ${new Date(props.updatesCheckedAt).toLocaleString()}. Pins stay unchanged until you refresh a reviewed source.`
                  : 'Check registered source refs when you are online. Pins stay unchanged.'}
              </Text>
              <Button
                action={COMMANDS_AND_AI_ACTIONS.checkGuidanceUpdates.value}
                disabled={props.updatesLoading || loading}
              >
                {props.updatesLoading ? 'Checking sources…' : 'Check Now'}
              </Button>
              {props.updatesError ? (
                <StateBanner
                  title="Source Check Failed"
                  detail={`${props.updatesError} Previous results may be stale.`}
                  tone="warning"
                  urgency="status"
                />
              ) : null}
              {overview.sources.map((source) => {
                const update = props.updates?.find((item) => item.id === source.id);
                const pendingRules = source.rules.filter(
                  (rule) => rule.selected && rule.installed_version !== rule.version,
                );
                return (
                  <div data-guidance-update-source={source.id}>
                    <List gap="xs">
                      {update?.state === 'revision_changed' && (
                        <StateBanner
                          title={`${source.id} Has A Different Source Revision`}
                          detail={`Pinned ${source.commit.slice(0, 12)}; observed ${update.observed_commit?.slice(0, 12)}. Review selected guidance before refreshing.`}
                          tone="info"
                          urgency="status"
                          copyLayout="stacked"
                        />
                      )}
                      {update?.state === 'tag_moved' && (
                        <StateBanner
                          title={`${source.id} Tag Moved`}
                          detail="This tag now points to a different commit. Verify the source manually before choosing a new pin."
                          tone="warning"
                          urgency="status"
                        />
                      )}
                      <Text>
                        {source.id}:{' '}
                        {update
                          ? {
                              current: 'source is current',
                              revision_changed: 'source revision changed',
                              pinned: 'source is pinned',
                              tag_moved: 'tag moved; review manually',
                              check_failed: 'source status is unknown',
                            }[update.state]
                          : 'not checked'}
                      </Text>
                      {update?.observed_commit ? (
                        <Text tone="quiet" size="compact">
                          Observed {update.observed_commit.slice(0, 12)} · pinned {source.commit.slice(0, 12)}
                        </Text>
                      ) : null}
                      {update?.reason ? (
                        <Text tone="quiet" size="compact">
                          {update.reason}
                        </Text>
                      ) : null}
                      {!!pendingRules.length && (
                        <Text tone="quiet" size="compact">
                          {pendingRules.length} selected rule(s) still need Apply Selected Guidance at the pinned
                          version.
                        </Text>
                      )}
                      {update?.state === 'revision_changed' && (
                        <Button
                          action={COMMANDS_AND_AI_ACTIONS.reviewGuidanceSource.value}
                          disabled={props.reviewLoading || loading}
                        >
                          Review Changes
                        </Button>
                      )}
                    </List>
                  </div>
                );
              })}
              {props.reviewError ? (
                <StateBanner title="Review Failed" detail={props.reviewError} tone="warning" urgency="status" />
              ) : null}
              {props.review && (
                <section aria-label={`${props.review.update.id} Update Review`} data-guidance-review="">
                  <List gap="xs">
                    <Text variant="h3">Review {props.review.update.id}</Text>
                    {props.review.update.state !== 'revision_changed' && (
                      <Text tone="quiet">
                        {
                          {
                            current: 'The source is current.',
                            pinned: 'This ref is pinned.',
                            tag_moved: 'The tag moved. Verify it manually before choosing a new pin.',
                            check_failed: 'The source could not be checked.',
                          }[props.review.update.state]
                        }
                      </Text>
                    )}
                    {props.review.update.state === 'revision_changed' && (
                      <Text tone="quiet" size="compact">
                        Pinned {props.review.update.pinned_commit} · candidate {props.review.update.observed_commit}
                      </Text>
                    )}
                    {props.review.update.state === 'revision_changed' &&
                      (props.review.changes.length ? (
                        props.review.changes.map((change) => (
                          <Text>
                            {change.kind} {change.name}: {change.change}
                            {change.before_version !== null && ` v${change.before_version}`}
                            {change.after_version !== null && ` → v${change.after_version}`}
                            {change.selected ? ' · selected' : ' · not selected'}
                          </Text>
                        ))
                      ) : (
                        <Text tone="quiet">No rule or skill content changed at this revision.</Text>
                      ))}
                    {props.review.update.state === 'revision_changed' && (
                      <Text tone="quiet" size="compact">
                        Refreshing replaces the pin after checking this exact commit again. Apply selected guidance
                        separately to install it; local edits remain protected.
                      </Text>
                    )}
                    {props.review.update.state === 'revision_changed' && removedSelected && (
                      <Text tone="quiet" size="compact">
                        This candidate removes selected guidance. Deselect and apply those entries at the old pin, then
                        review the source again.
                      </Text>
                    )}
                    {props.review.update.state === 'revision_changed' && (
                      <Button
                        action={COMMANDS_AND_AI_ACTIONS.refreshReviewedGuidance.value}
                        disabled={loading || props.reviewLoading || removedSelected}
                      >
                        Refresh Reviewed Pin
                      </Button>
                    )}
                  </List>
                </section>
              )}
            </List>
          </section>
        )}
        <Grid minColumnWidth={rem(14)} gap="m">
          <TextField name="guidance-source-id" label="Source ID" value={sourceId} />
          <TextField
            name="guidance-repository"
            label="Rule Repository"
            value={repository || overview?.default_repository || ''}
          />
          <TextField name="guidance-ref" label="Tag or Commit" value={sourceRef} />
        </Grid>
        <Button
          action={
            registeredSource
              ? COMMANDS_AND_AI_ACTIONS.reviewGuidanceSource.value
              : COMMANDS_AND_AI_ACTIONS.registerGuidanceSource.value
          }
          rootAttributes={registeredSource ? { 'data-guidance-update-source': sourceId } : undefined}
          disabled={loading}
        >
          {registeredSource ? 'Review Pinned Source' : 'Register source'}
        </Button>
        {overview?.sources.length ? (
          <List gap="l">
            <Grid minColumnWidth={rem(12)} gap="m">
              <TextField name="guidance-search" label="Search Rules" value={query} />
              <Select
                name="guidance-owner-filter"
                label="Owner"
                value={ownerFilter}
                choices={[{ value: '', label: 'All Owners' }, ...owners.map((value) => ({ value, label: value }))]}
              />
              <Select
                name="guidance-tag-filter"
                label="Technology or Tag"
                value={tagFilter}
                choices={[{ value: '', label: 'All Tags' }, ...tags.map((value) => ({ value, label: value }))]}
              />
            </Grid>
            {overview.sources.map((source) => {
              const visible = source.rules.filter(
                (rule) =>
                  (!query || `${rule.name} ${rule.description}`.toLowerCase().includes(query.toLowerCase())) &&
                  (!ownerFilter || rule.owner === ownerFilter) &&
                  (!tagFilter || rule.tags.includes(tagFilter)),
              );
              const groups = new Map<string, typeof visible>();
              for (const rule of visible) {
                const owner = rule.owner || 'Other rules';
                const group = groups.get(owner) ?? [];
                group.push(rule);
                groups.set(owner, group);
              }
              return (
                <section data-guidance-source={source.id} aria-label={`${source.id} Rules`}>
                  <List gap="m">
                    <Text variant="h3">{source.id}</Text>
                    <Text tone="quiet" size="compact">
                      Pinned to {source.commit.slice(0, 12)} from {source.repository}
                    </Text>
                    {[...groups.entries()]
                      .sort(([left], [right]) => left.localeCompare(right))
                      .map(([owner, group]) => (
                        <Disclosure
                          title={`${owner} · ${group.length} ${group.length === 1 ? 'rule' : 'rules'}`}
                          detail={`${group.filter((rule) => rule.selected).length} selected`}
                          open
                        >
                          <List gap="m">
                            {group.map((rule) => {
                              const preview =
                                props.rulePreview?.source === source.id && props.rulePreview.rule === rule.name
                                  ? props.rulePreview
                                  : undefined;
                              return (
                                <section
                                  data-guidance-rule={rule.name}
                                  data-guidance-source={source.id}
                                  aria-label={rule.description}
                                >
                                  <List gap="xs">
                                    <Checkbox
                                      action={COMMANDS_AND_AI_ACTIONS.toggleGuidanceRule.value}
                                      checked={rule.selected}
                                      label={rule.description}
                                      hint={`${rule.name} · available v${rule.version} · ${rule.installed_version === null ? 'not installed' : `v${rule.installed_version} installed`}`}
                                    />
                                    {!!rule.tags.length && (
                                      <div aria-label={`${rule.name} tags`}>
                                        {rule.tags.map((tag) => (
                                          <Badge tone="neutral" size="compact">
                                            {tag}
                                          </Badge>
                                        ))}
                                      </div>
                                    )}
                                    <div>
                                      <Button
                                        action={COMMANDS_AND_AI_ACTIONS.previewGuidanceRule.value}
                                        disabled={loading}
                                      >
                                        {preview ? 'Close Preview' : 'Preview Rule and Changes'}
                                      </Button>
                                    </div>
                                    {preview?.loading && <Text tone="quiet">Loading pinned rule preview…</Text>}
                                    {preview?.error ? (
                                      <StateBanner
                                        title="Rule Preview Unavailable"
                                        detail={preview.error}
                                        tone="warning"
                                        urgency="status"
                                      />
                                    ) : null}
                                    {preview?.result && (
                                      <section aria-label={`${rule.name} Change Preview`}>
                                        <List gap="xs">
                                          <Text>
                                            {
                                              {
                                                add: 'Applying this selection adds the pinned rule.',
                                                replace:
                                                  'Applying this selection replaces the installed rule with the pinned version.',
                                                current: 'The pinned rule is already installed.',
                                                protected:
                                                  'Local edits or a newer installed version are protected; applying will not replace this rule.',
                                              }[preview.result.outcome]
                                            }
                                          </Text>
                                          <Text tone="quiet" size="compact">
                                            {preview.result.owner === 'rulesync'
                                              ? 'Rulesync input preview'
                                              : 'AGENTS.md section preview'}{' '}
                                            · Read-only
                                          </Text>
                                          <Grid minColumnWidth={rem(14)} gap="m">
                                            <TextField
                                              label="Currently Installed"
                                              readOnly
                                              multiline
                                              rows={8}
                                              value={preview.result.current ?? '(not installed)'}
                                            />
                                            <TextField
                                              label="After Apply"
                                              readOnly
                                              multiline
                                              rows={8}
                                              value={preview.result.after ?? '(not installed)'}
                                            />
                                          </Grid>
                                          {preview.result.outcome === 'protected' && (
                                            <TextField
                                              label="Pinned Rule Offered"
                                              readOnly
                                              multiline
                                              rows={8}
                                              value={preview.result.pinned}
                                            />
                                          )}
                                        </List>
                                      </section>
                                    )}
                                  </List>
                                </section>
                              );
                            })}
                          </List>
                        </Disclosure>
                      ))}
                    {!visible.length && <Text tone="quiet">No rules match these filters.</Text>}
                    {source.skills.map((skill) => (
                      <div data-guidance-skill={skill.name} data-guidance-source={source.id}>
                        <Checkbox
                          action={COMMANDS_AND_AI_ACTIONS.toggleGuidanceSkill.value}
                          checked={skill.selected}
                          label={`Skill: ${skill.name}`}
                        />
                      </div>
                    ))}
                  </List>
                </section>
              );
            })}
            <Button action={COMMANDS_AND_AI_ACTIONS.applyGuidance.value} tone="brand" disabled={loading}>
              Apply Selected Guidance
            </Button>
          </List>
        ) : (
          <Text tone="quiet">Register a source to browse its rules and skills.</Text>
        )}
        <Text role="status" tone="quiet">
          {message}
        </Text>
      </List>
    </section>
  );
}
