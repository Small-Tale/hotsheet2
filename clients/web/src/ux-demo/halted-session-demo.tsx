import { List } from '@kerfjs/ui/list';
import { Text } from '@kerfjs/ui/text';
import { signal } from 'kerfjs';

import { HaltedSessionPopup } from '../components/halted-session-popup';

export const haltedSessionDemoVisible = signal(true),
  haltedSessionDemoResult = signal('The session remains halted after dismissing this prompt.');

export function HaltedSessionPopupDemo() {
  return (
    <List gap="m">
      <Text>{haltedSessionDemoResult.value}</Text>
      <button type="button" data-reset-halt-demo>
        Show halted session
      </button>
      {haltedSessionDemoVisible.value && (
        <HaltedSessionPopup
          episode={{
            key: 'demo-halt',
            kind: 'terminal',
            projectId: 'demo',
            projectName: 'Hot Sheet demo',
            sessionId: 'claude-main',
            sessionName: 'Claude main worker',
            message: 'Selected model is at capacity. Please try a different model.',
          }}
        />
      )}
    </List>
  );
}
