import {
  NOTIFICATIONS_AND_LINKS_ACTIONS,
  NOTIFICATIONS_AND_LINKS_TARGETS,
} from './interaction-attrs/notifications-and-links';

/** Return to the logical opener after a controlled editor is removed by a render. */
export function createPermissionRuleEditorFocus(root: ParentNode, context: () => string = () => '') {
  let origin: { node: HTMLElement; key: string; surface: string; index: number; context: string } | undefined;
  const surfaceFor = (node: Element) =>
    node.closest('[data-permission-selection-surface]')?.getAttribute('data-permission-selection-surface') ?? 'preview';
  const openers = (surface: string) =>
    [...root.querySelectorAll<HTMLElement>(NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRuleRow.selector)].filter(
      (node) => surfaceFor(node) === surface,
    );
  return {
    remember(node: HTMLElement) {
      const surface = surfaceFor(node);
      const key = node.getAttribute('data-rule-key') ?? '';
      const row = openers(surface).find((candidate) => candidate.getAttribute('data-rule-key') === key) ?? node;
      origin = {
        node: row,
        key,
        surface,
        index: Math.max(0, openers(surface).indexOf(row)),
        context: context(),
      };
    },
    snapshot: () => origin,
    dispose: () => {
      origin = undefined;
    },
    restore(saved = origin) {
      requestAnimationFrame(() => {
        if (
          !saved ||
          saved !== origin ||
          saved.context !== context() ||
          root.querySelector(NOTIFICATIONS_AND_LINKS_TARGETS.permissionRuleEditor.selector)
        )
          return;
        const candidates = openers(saved.surface);
        const target =
          candidates.find((node) => node.getAttribute('data-rule-key') === saved.key) ??
          (saved.node.isConnected ? saved.node : candidates.at(Math.min(saved.index, candidates.length - 1)));
        if (target?.isConnected && !target.hasAttribute('disabled')) target.focus({ preventScroll: true });
      });
    },
  };
}
