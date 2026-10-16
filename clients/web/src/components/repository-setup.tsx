import './repository-setup.css';

import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { CircleCheck } from 'lucide';

import { REPOSITORY_ACTIONS } from '../interaction-attrs/repository';

export type RepositorySetupStep = 'initialize' | 'remote';

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
        <wa-input
          name="repository-remote"
          type="text"
          label="Remote URL"
          placeholder="git@github.com:you/project.git"
          required
          autofocus
        />
        {error && <StateBanner tone="danger" urgency="alert" title={error} />}
        <Row hAlign="right" gap="xs" wrap>
          <wa-button
            appearance="plain"
            type="button"
            {...REPOSITORY_ACTIONS.skipRepositoryRemote.attrs}
            disabled={busy}
          >
            Skip for Now
          </wa-button>
          <wa-button appearance="accent" type="submit" disabled={busy}>
            {busy ? 'Adding origin…' : 'Add origin'}
          </wa-button>
        </Row>
      </form>
    );

  return (
    <div class="repository-setup" data-component="repository-setup" data-step="initialize">
      <Text tone="quiet" align="center" flush>
        Initialize Git here to enable branch, change, and commit status. Existing project files will remain untracked;
        Hot Sheet will not stage or commit them.
      </Text>
      {error && <StateBanner tone="danger" urgency="alert" title={error} />}
      <Row hAlign="center">
        <wa-button appearance="accent" type="button" {...REPOSITORY_ACTIONS.initializeRepository.attrs} disabled={busy}>
          {busy ? 'Initializing…' : 'Initialize Git repository'}
        </wa-button>
      </Row>
    </div>
  );
}
