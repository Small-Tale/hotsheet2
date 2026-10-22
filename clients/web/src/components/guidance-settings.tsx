import { Button } from '@kerfjs/ui/button';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import type { GuidanceOverview } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

export interface GuidanceSettingsProps {
  overview?: GuidanceOverview;
  loading: boolean;
  message: string;
  query: string;
  repository: string;
  sourceRef: string;
  sourceId: string;
  ownerFilter: string;
  tagFilter: string;
}

/** Project Settings → AI tools: discover and apply pinned guidance (HS2-2RD1M1). */
export function GuidanceSettings(props: GuidanceSettingsProps) {
  const { overview, loading, message, query, repository, sourceRef, sourceId, ownerFilter, tagFilter } = props;
  const rules = overview?.sources.flatMap((source) => source.rules) ?? [];
  const owners = [
    ...new Set(rules.map((rule) => rule.owner).filter((owner): owner is string => Boolean(owner))),
  ].sort();
  const tags = [...new Set(rules.flatMap((rule) => rule.tags))].sort();
  return (
    <section data-component="guidance-settings" aria-label="Project Guidance" aria-busy={String(loading)}>
      <List gap="l">
        <List gap="xs">
          <Text variant="h2">Project guidance</Text>
          <Text tone="quiet">Choose a pinned rule source and the guidance this project receives.</Text>
        </List>
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
          <Text tone="quiet">
            Rulesync regenerates <code>AGENTS.md</code> from its rule inputs. Move project-local guidance into{' '}
            <code>.rulesync/rules</code> before applying.
          </Text>
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
        <Button action={COMMANDS_AND_AI_ACTIONS.registerGuidanceSource.value} disabled={loading}>
          {overview?.sources.some((source) => source.id === sourceId) ? 'Refresh pinned source' : 'Register source'}
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
              return (
                <section data-guidance-source={source.id} aria-label={`${source.id} Rules`}>
                  <List gap="xs">
                    <Text variant="h3">{source.id}</Text>
                    <Text tone="quiet" size="compact">
                      Pinned to {source.commit.slice(0, 12)} from {source.repository}
                    </Text>
                    {visible.map((rule) => (
                      <div data-guidance-rule={rule.name} data-guidance-source={source.id}>
                        <Checkbox
                          action={COMMANDS_AND_AI_ACTIONS.toggleGuidanceRule.value}
                          checked={rule.selected}
                          label={`${rule.name} · available v${rule.version}`}
                          hint={`${rule.description} · installed ${rule.installed_version === null ? 'not yet' : `v${rule.installed_version}`}`}
                        />
                      </div>
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
