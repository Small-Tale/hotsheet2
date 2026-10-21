import './repository-setup.css';

import { Button } from '@kerfjs/ui/button';
import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { CircleCheck } from 'lucide';

import { REPOSITORY_ACTIONS } from '../interaction-attrs/repository';
import type { RepositorySetupStep } from '../repository-state';

export type { RepositorySetupStep } from '../repository-state';

export function RepositorySetup({
  step = 'initialize',
  busy = false,
  error = '',
}: {
  step?: RepositorySetupStep;
  busy?: boolean;
  error?: string;
}) {
  if (step === 'remote')
    return (
      <form
        class="repository-setup"
        data-component="repository-setup"
        data-step="remote"
        {...REPOSITORY_ACTIONS.connectRepositoryRemote.attrs}
      >
        <List contentWidth="compact" inset="document" gap="l">
          <Row gap="m" vAlign="top">
            <span class="repository-setup__icon">
              <LucideIcon size="m" icon={CircleCheck} name="circle-check" color={uiColor('brand-on-quiet')} />
            </span>
            <List gap="xs" flex>
              <Text variant="h2" size="large" flush lineHeight="tight">
                Git is ready
              </Text>
              <Text tone="quiet" flush>
                Add an <code>origin</code> remote now, or skip this step and configure one later. Hot Sheet will not
                stage, commit, or push project files.
              </Text>
            </List>
          </Row>
          <TextField
            name="repository-remote"
            type="text"
            label="Remote URL"
            placeholder="git@github.com:you/project.git"
            required
            autofocus
          />
          {error ? <StateBanner tone="danger" urgency="alert" title={error} /> : undefined}
          <Row hAlign="right" gap="xs" wrap>
            <Button
              emphasis="plain"
              type="button"
              action={REPOSITORY_ACTIONS.skipRepositoryRemote.value}
              disabled={busy}
            >
              Skip for Now
            </Button>
            <Button emphasis="primary" type="submit" disabled={busy}>
              {busy ? 'Adding origin…' : 'Add origin'}
            </Button>
          </Row>
        </List>
      </form>
    );

  return (
    <div class="repository-setup" data-component="repository-setup" data-step="initialize">
      <List contentWidth="compact" inset="document" gap="l">
        <Text tone="quiet" align="center" flush>
          Initialize Git here to enable branch, change, and commit status. Existing project files will remain untracked;
          Hot Sheet will not stage or commit them.
        </Text>
        {error ? <StateBanner tone="danger" urgency="alert" title={error} /> : undefined}
        <Row hAlign="center">
          <Button
            emphasis="primary"
            type="button"
            action={REPOSITORY_ACTIONS.initializeRepository.value}
            disabled={busy}
          >
            {busy ? 'Initializing…' : 'Initialize Git repository'}
          </Button>
        </Row>
      </List>
    </div>
  );
}
