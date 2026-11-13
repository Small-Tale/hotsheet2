import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';

/** Desktop-only choice. A phone browser continues to use the existing one-host picker. */
export function ProjectSourceChoiceDialog({ open }: { open: boolean }) {
  return (
    <div class="project-source-choice-dialog">
      <DialogSurface
        label="Open a project"
        rootAttributes={{
          'data-project-source-choice': '',
          'data-controlled-open': String(open),
        }}
      >
        <List gap="xs">
          <Text tone="quiet">Where is the project hosted?</Text>
          <Button emphasis="secondary" action={PROJECT_LIFECYCLE_ACTIONS.chooseLocalProject.value}>
            Local Project
          </Button>
          <Button emphasis="secondary" action={PROJECT_LIFECYCLE_ACTIONS.chooseDesktopRemoteProject.value}>
            Remote Project
          </Button>
          <Button emphasis="plain" action={PROJECT_LIFECYCLE_ACTIONS.cancelProjectSourceChoice.value}>
            Cancel
          </Button>
        </List>
      </DialogSurface>
    </div>
  );
}
