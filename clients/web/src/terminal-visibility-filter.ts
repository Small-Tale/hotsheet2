import { delegate } from 'kerfjs';

import { type TerminalVisibilityType, terminalVisibilityTypes } from './terminal-visibility';

interface TypeSelect extends HTMLElement {
  value: string | string[] | null;
}
const selector = 'wa-select[data-terminal-type-filter]';

/** Bind a controlled array to the native multi-select island without morphing its open menu. */
export function wireTerminalVisibilityTypeFilter(
  root: HTMLElement,
  onChange: (types: TerminalVisibilityType[]) => void,
): () => void {
  const sync = () => {
    for (const select of root.querySelectorAll<TypeSelect>(selector)) {
      const types = terminalVisibilityTypes(
        (select.closest<HTMLElement>('[data-selected-types]')?.dataset.selectedTypes ?? '').split(','),
      );
      if (JSON.stringify(select.value) !== JSON.stringify(types)) select.value = types;
    }
  };
  const stop = delegate(root, 'change', selector, (_event, target) => {
    const select = target as TypeSelect;
    const types = terminalVisibilityTypes(Array.isArray(select.value) ? select.value : []);
    select.value = types;
    onChange(types);
  });
  const observer = new MutationObserver(sync);
  observer.observe(root, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['data-selected-types'],
  });
  void customElements.whenDefined('wa-select').then(sync);
  sync();
  return () => {
    observer.disconnect();
    stop();
  };
}
