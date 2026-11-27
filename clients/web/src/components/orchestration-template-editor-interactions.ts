import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { OrchestrationTemplateEditing } from '../features/orchestration-template-editing';
import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import type { EditorPart } from '../orchestration-template-editor';
import { TEMPLATE_FIELD_PREFIX } from './orchestration-template-editor';

const EDITOR = '[data-dialog-component="orchestration-template-editor"]';
const FIELD = `${EDITOR} [name^="${TEMPLATE_FIELD_PREFIX}"]`;

export interface TemplateEditorWiring {
  /** The project whose settings show: the orchestration project. */
  projectId: () => string | undefined;
  /** The declared project a new template's first step runs in. */
  firstProject?: (projectId: string) => string | undefined;
}

type FieldElement = Element & { value?: unknown; checked?: boolean; type?: string };

const fieldValue = (target: FieldElement): string | boolean =>
  target.type === 'checkbox' ? target.checked === true : typeof target.value === 'string' ? target.value : '';
const fieldName = (target: Element) => (target.getAttribute('name') ?? '').slice(TEMPLATE_FIELD_PREFIX.length);
const isText = (target: Element) => target.localName === 'input' || target.localName === 'textarea';

/**
 * HS2-1XCW6N: delegated handlers for Settings › Orchestration › Templates and the template editor. A
 * text field updates the draft on each keystroke and is checked when it commits (change, or leaving it,
 * since WebKit fires no change once a re-render wrote the same value back); a select or checkbox commits
 * at once. The application and the catalog demo install the same handlers over the same controller.
 */
export function wireOrchestrationTemplateEditor(
  root: HTMLElement,
  editing: OrchestrationTemplateEditing,
  wiring: TemplateEditorWiring,
): () => void {
  const lifetime = createScope();
  const withProject = (run: (projectId: string) => void) => {
    const projectId = wiring.projectId();
    if (projectId) run(projectId);
  };
  const templateOf = (target: Element) =>
    target.getAttribute('data-template-id') ?? target.getAttribute('data-item-id') ?? undefined;
  const partOf = (target: Element) =>
    (target.closest<HTMLElement>('[data-template-part]')?.dataset.templatePart ?? undefined) as EditorPart | undefined;
  lifetime.add(
    delegate(root, 'click', ACTIONS.newTemplate.selector, () => {
      withProject((projectId) => {
        editing.openNew(projectId, wiring.firstProject?.(projectId) ?? '');
      });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.editTemplate.selector, (_event, target) => {
      const id = templateOf(target);
      if (id)
        withProject((projectId) => {
          editing.openEdit(projectId, id);
        });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.retireTemplate.selector, (_event, target) => {
      const id = templateOf(target);
      if (id) withProject((projectId) => void editing.retire(projectId, id));
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.selectTemplatePart.selector, (_event, target) => {
      const part = partOf(target);
      if (part) editing.select(part);
    }),
  );
  for (const type of ['input', 'change'] as const)
    lifetime.add(
      delegate(root, type, FIELD, (_event, target) => {
        const field = target as FieldElement,
          name = fieldName(field);
        // A checkbox or select only changes; a text field reports each keystroke and commits on change.
        if (!name || (type === 'input' && !isText(field))) return;
        editing.edit(name, fieldValue(field), type === 'change');
      }),
    );
  lifetime.add(
    delegate(root, 'focusout', FIELD, (_event, target) => {
      const field = target as FieldElement,
        name = fieldName(field);
      if (name && isText(field) && field.type !== 'checkbox') editing.edit(name, fieldValue(field), true);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.addTemplateInput.selector, () => {
      editing.addInput();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.addTemplateStep.selector, () => {
      editing.addStep();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.removeTemplatePart.selector, (_event, target) => {
      const part = partOf(target);
      if (part) editing.removePart(part);
    }),
  );
  for (const [spec, by] of [
    [ACTIONS.moveTemplatePartUp, -1],
    [ACTIONS.moveTemplatePartDown, 1],
  ] as const)
    lifetime.add(
      delegate(root, 'click', spec.selector, (_event, target) => {
        const part = partOf(target);
        if (part) editing.move(part, by);
      }),
    );
  lifetime.add(
    delegate(root, 'click', ACTIONS.addTemplateAfter.selector, (_event, target) => {
      const step = target.getAttribute('data-step-id');
      if (step) editing.addPrerequisite(step);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.removeTemplateAfter.selector, (_event, target) => {
      const step = target.getAttribute('data-step-id'),
        index = Number(target.getAttribute('data-index'));
      if (step && Number.isInteger(index)) editing.removePrerequisite(step, index);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.saveTemplate.selector, () => {
      void editing.save();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.reloadTemplate.selector, () => {
      void editing.reload();
    }),
  );
  lifetime.add(
    delegate(root, 'click', '[data-nav-back]', (_event, target) => {
      if (target.closest(EDITOR)) editing.back();
    }),
  );
  lifetime.add(
    delegate(root, 'kui-request-close', EDITOR, (event, target) => {
      // Only the dialog's own close request counts (Escape, the close control, Cancel and Close).
      if (event.target === target) void editing.close();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
