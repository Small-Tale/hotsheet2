import { delegate } from 'kerfjs';

import { type TerminalVisibilityType, terminalVisibilityTypes } from './terminal-visibility';

interface TypeSelect extends HTMLElement {
  value: string | string[] | null;
}
const selector = 'wa-select[name="terminal-visibility-types"]';

/**
 * Report the controlled Kerf multiple Select's value changes, including its Select all / Clear
 * footer actions, which Kerf's register module turns into ordinary `change` events.
 */
export function wireTerminalVisibilityTypeFilter(
  root: HTMLElement,
  onChange: (types: TerminalVisibilityType[]) => void,
): () => void {
  return delegate(root, 'change', selector, (_event, target) => {
    const select = target as TypeSelect;
    onChange(terminalVisibilityTypes(Array.isArray(select.value) ? select.value : []));
  });
}
