import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { createAiLaunchDialog } from './ai-launch-dialog';
import type { createCommandGroupDialog } from './command-group-dialog';
import { INPUT_DIALOG_ACTIONS as actions, INPUT_DIALOG_TARGETS as targets } from './interaction-attrs/input-dialogs';

/** The same delegates serve production and the component catalog (HS2-WMZ6Z5). */
export function wireInputDialogs(
  root: HTMLElement,
  dialogs: {
    ai: ReturnType<typeof createAiLaunchDialog>;
    group: ReturnType<typeof createCommandGroupDialog>;
  },
) {
  const scope = createScope();
  function cancel(kind: 'ai' | 'group') {
    dialogs[kind].cancel();
    // The AI menu item is transient; return to its persistent drawer trigger instead.
    queueMicrotask(() => {
      if (scope.signal.aborted || dialogs[kind].state.value) return;
      const visible = (node: HTMLElement | undefined) =>
        Boolean(node?.isConnected && node.getClientRects().length > 0 && !node.hasAttribute('disabled'));
      const fallback =
        kind === 'group'
          ? '[data-action="add-command-group"], [data-action="demo-open-command-group"]'
          : '[data-terminal-drawer-create] > button[aria-haspopup="menu"], [data-action="demo-open-ai-launch"]';
      const target = Array.from(root.querySelectorAll<HTMLElement>(fallback)).find(visible);
      target?.focus({ preventScroll: true });
    });
  }
  scope.add(
    delegate(root, 'input', targets.groupName.selector, (_event, field) => {
      dialogs.group.input((field as HTMLInputElement).value);
    }),
  );
  scope.add(
    delegate(root, 'submit', actions.submitCommandGroup.selector, (event) => {
      event.preventDefault();
      dialogs.group.submit();
    }),
  );
  scope.add(
    delegate(root, 'click', actions.cancelCommandGroup.selector, () => {
      cancel('group');
    }),
  );
  scope.add(
    delegateCapture(root, 'kui-request-close', targets.group.selector, () => {
      cancel('group');
    }),
  );
  for (const field of ['provider', 'model'] as const) {
    scope.add(
      delegate(root, 'input', targets[field].selector, (_event, input) => {
        dialogs.ai.input(field, (input as HTMLInputElement).value);
      }),
    );
  }
  scope.add(
    delegate(root, 'change', targets.effort.selector, (_event, field) => {
      const value = (field as HTMLInputElement).value;
      dialogs.ai.input('effort', value === '__provider-default__' ? '' : value);
    }),
  );
  scope.add(
    delegate(root, 'submit', actions.submitAiLaunch.selector, (event) => {
      event.preventDefault();
      dialogs.ai.submit();
    }),
  );
  scope.add(
    delegate(root, 'click', actions.cancelAiLaunch.selector, () => {
      cancel('ai');
    }),
  );
  scope.add(
    delegateCapture(root, 'kui-request-close', targets.ai.selector, () => {
      cancel('ai');
    }),
  );
  return () => {
    dialogs.ai.cancel();
    dialogs.group.cancel();
    scope.dispose();
  };
}
