import { type ComboboxCommit, wireComboboxes } from '@kerfjs/ui/wire-comboboxes';

export const TICKET_TAG_COMBOBOX_COMMIT = 'ticket-tag-combobox-commit';
/** HS2-1X1TXC, HS2-NPAZQW: a model or resource chosen in an orchestration Limits field. */
export const ORCHESTRATION_PATTERN_COMMIT = 'orchestration-pattern-commit';
/** The id prefix of those fields' comboboxes (`OrchestrationPatternField`). */
const ORCHESTRATION_PATTERN_PREFIX = 'orchestration-limits-';

/** One Combobox listener per root; tag commits reach the tag editor through a bubbling event. */
export function wireAppComboboxes(root: HTMLElement | Document): () => void {
  // Provider repository names do not use comma as a commit key; retain typed punctuation.
  const keepProviderComma = (event: Event) => {
    const keyboard = event as KeyboardEvent;
    if (
      keyboard.key === ',' &&
      keyboard.target instanceof HTMLInputElement &&
      keyboard.target.matches('[data-component="combobox"] input') &&
      !keyboard.target.closest('[data-combobox-id^="ticket-tag-"]') &&
      !keyboard.target.closest(`[data-combobox-id^="${ORCHESTRATION_PATTERN_PREFIX}"]`)
    )
      keyboard.stopPropagation();
  };
  root.addEventListener('keydown', keepProviderComma, true);
  const dispose = wireComboboxes(root, {
    onCommit: ({ id, input, value }: ComboboxCommit) => {
      if (id.startsWith('ticket-tag-')) {
        input.value = '';
        input.dispatchEvent(new CustomEvent(TICKET_TAG_COMBOBOX_COMMIT, { bubbles: true, detail: value }));
      } else if (id.startsWith(ORCHESTRATION_PATTERN_PREFIX)) {
        // Each chosen model or resource becomes a chip; the field clears for the next one.
        input.value = '';
        input.dispatchEvent(new CustomEvent(ORCHESTRATION_PATTERN_COMMIT, { bubbles: true, detail: value }));
      } else {
        // Provider setup submits its chosen or typed repository by the input's name.
        input.value = value;
      }
    },
  });
  return () => {
    dispose();
    root.removeEventListener('keydown', keepProviderComma, true);
  };
}
