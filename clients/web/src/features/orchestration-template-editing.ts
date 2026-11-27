import { signal } from 'kerfjs';

import type { Confirm } from '../confirmation';
import { describeRefusal } from '../orchestration-refusals';
import {
  addAfter,
  addInput,
  addStep,
  draftChanged,
  draftFromTemplate,
  editDraft,
  type EditorPart,
  localProblems,
  movePart,
  newTemplateDraft,
  partExists,
  removeAfter,
  removeInput,
  removeStep,
  type TemplateDraft,
  templateFromDraft,
} from '../orchestration-template-editor';
import type { TemplateProblem } from '../orchestration-templates';

type Raw = Record<string, unknown>;

/** The template routes the editor uses (HS2-1XCW6N): list, validate, set and retire. */
export interface TemplateEditingApi {
  orchestrationTemplates(
    checkout: string,
    operation: 'list' | 'validate' | 'set' | 'retire',
    body: Record<string, unknown>,
    actorId: string,
  ): Promise<Raw>;
}

export interface TemplateEditingDependencies {
  api: (projectId: string) => TemplateEditingApi | undefined;
  /** The person acting, for each request's actor. */
  personId: () => string;
  confirm: Confirm;
  /** HS2-1K6122: the app's toast, for each change's outcome. */
  notify?: (message: string) => void;
  /**
   * Run after a typing pause (`TYPING_PAUSE_MS`), answering a cancel; tests pass their own. A pause checks
   * the draft too, since WebKit fires neither change nor focusout for a field a re-render wrote back.
   */
  afterPause?: (run: () => void) => () => void;
}

/** The typing pause after which the draft is checked, as Use Template… reads its summary again. */
export const TYPING_PAUSE_MS = 400;

/** Settings › Orchestration › Templates of one project: the checked-in templates as the server lists them. */
export interface TemplateListState {
  loading: boolean;
  /** A read failure, in words. */
  error?: string;
  /** The templates file's revision, absent while nothing is checked in. */
  revision?: string;
  templates: readonly Raw[];
  /** Templates with a retire on its way. */
  busy: readonly string[];
}

/** The open template editor. */
export interface TemplateEditorState {
  projectId: string;
  /** The revision the draft was opened from; a save is refused if the file changed since. */
  revision?: string;
  original: TemplateDraft;
  draft: TemplateDraft;
  part: EditorPart;
  /** On a phone: the part's detail is pushed over the list. */
  detailActive: boolean;
  /** The validator's problems of the draft as last checked, with the local ones. */
  problems: readonly TemplateProblem[];
  validating: boolean;
  /** A save is on its way. */
  busy: boolean;
  /** A refused save, in words. */
  failure?: string;
  /** The save was refused because the templates changed elsewhere since the editor opened. */
  conflict: boolean;
  /** Increments per open, so fields never show a previous session's text. */
  session: number;
}

const EMPTY_LIST: TemplateListState = { loading: false, templates: [], busy: [] };
const codeOf = (reason: unknown) =>
  reason && typeof reason === 'object' && 'code' in reason ? String(reason.code) : undefined;
const problemsOf = (value: unknown): TemplateProblem[] =>
  Array.isArray(value)
    ? value.flatMap((item: unknown) => {
        const raw = item && typeof item === 'object' ? (item as Raw) : {};
        return typeof raw.code === 'string' ? [{ code: raw.code, at: typeof raw.at === 'string' ? raw.at : '' }] : [];
      })
    : [];
const nameOf = (template: Raw | TemplateDraft) =>
  (typeof template.name === 'string' && template.name.trim()) || 'the template';

/**
 * HS2-1XCW6N: the controller of Settings › Orchestration › Templates and its editor. It reads the list
 * when the category shows, opens a template (or a new one) as a draft, checks the draft with the server's
 * validator on each committed change, and saves it whole through `templates/set` with the revision it was
 * opened from. Retire asks first. Every change's outcome is a toast; a refused save also keeps its banner.
 */
export function createOrchestrationTemplateEditing(dependencies: TemplateEditingDependencies) {
  const lists = signal<Record<string, TemplateListState | undefined>>({}),
    editor = signal<TemplateEditorState | undefined>(undefined);
  let sessions = 0,
    validations = 0,
    cancelPause: (() => void) | undefined;
  const afterPause =
    dependencies.afterPause ??
    ((run: () => void) => {
      const timer = setTimeout(run, TYPING_PAUSE_MS);
      return () => {
        clearTimeout(timer);
      };
    });
  const readGenerations = new Map<string, number>();

  const listOf = (projectId: string) => lists.value[projectId] ?? EMPTY_LIST;
  const putList = (projectId: string, change: Partial<TemplateListState>) => {
    lists.value = { ...lists.value, [projectId]: { ...listOf(projectId), ...change } };
  };
  const putEditor = (change: Partial<TemplateEditorState>) => {
    if (editor.value) editor.value = { ...editor.value, ...change };
  };
  const call = (projectId: string, operation: 'list' | 'validate' | 'set' | 'retire', body: Raw) => {
    const api = dependencies.api(projectId);
    if (!api) return Promise.reject(new Error('This project is not open.'));
    return api.orchestrationTemplates(projectId, operation, body, dependencies.personId());
  };

  /** Read the project's templates. An older answer never overwrites a newer one. */
  async function load(projectId: string) {
    const generation = (readGenerations.get(projectId) ?? 0) + 1;
    readGenerations.set(projectId, generation);
    putList(projectId, { loading: listOf(projectId).templates.length === 0 });
    try {
      const answer = await call(projectId, 'list', {});
      if (readGenerations.get(projectId) !== generation) return;
      putList(projectId, {
        loading: false,
        error: undefined,
        revision: typeof answer.revision === 'string' ? answer.revision : undefined,
        templates: Array.isArray(answer.templates)
          ? answer.templates.filter((item): item is Raw => Boolean(item) && typeof item === 'object')
          : [],
      });
    } catch (reason) {
      if (readGenerations.get(projectId) !== generation) return;
      putList(projectId, { loading: false, error: describeRefusal(reason, 'Could not read the templates') });
    }
  }

  function open(projectId: string, draft: TemplateDraft) {
    cancelPause?.();
    cancelPause = undefined;
    sessions += 1;
    editor.value = {
      projectId,
      revision: listOf(projectId).revision,
      original: draft,
      draft,
      part: 'template',
      detailActive: false,
      problems: localProblems(draft),
      validating: false,
      busy: false,
      conflict: false,
      session: sessions,
    };
  }

  /** New Template: a blank draft with one step in the first project. */
  function openNew(projectId: string, firstProject = '') {
    open(projectId, newTemplateDraft(firstProject));
  }

  /** Edit a checked-in template; a retired one opens read-only (the dialog disables its fields). */
  function openEdit(projectId: string, templateId: string) {
    const template = listOf(projectId).templates.find((candidate) => candidate.template_id === templateId);
    if (template) open(projectId, draftFromTemplate(template));
  }

  function select(part: EditorPart) {
    const state = editor.value;
    if (state && partExists(state.draft, part)) putEditor({ part, detailActive: true });
  }

  function back() {
    putEditor({ detailActive: false });
  }

  /** Check the draft with the server's validator. A later check wins over an earlier answer. */
  async function validate(): Promise<TemplateProblem[] | undefined> {
    const state = editor.value;
    if (!state) return undefined;
    const generation = ++validations,
      draft = state.draft;
    putEditor({ validating: true });
    try {
      const answer = await call(state.projectId, 'validate', { template: templateFromDraft(draft) });
      const problems = [...localProblems(draft), ...problemsOf(answer.problems)];
      if (generation === validations && editor.value?.session === state.session)
        putEditor({ problems, validating: false });
      return problems;
    } catch (reason) {
      // A draft the server cannot even read (such as a malformed field) is refused as a request.
      if (generation === validations && editor.value?.session === state.session)
        putEditor({ validating: false, failure: describeRefusal(reason, 'Could not check the template') });
      return undefined;
    }
  }

  function change(draft: TemplateDraft, part?: EditorPart) {
    const state = editor.value;
    if (!state) return;
    putEditor({
      draft,
      ...(part ? { part, detailActive: true } : partExists(draft, state.part) ? {} : { part: 'template' as const }),
      failure: undefined,
    });
  }

  /** A field changed; `commit` (a change event, not a keystroke) checks the draft again. */
  function edit(field: string, value: string | boolean, commit: boolean) {
    const state = editor.value;
    if (!state || state.draft.state === 'retired') return;
    const draft = editDraft(state.draft, field, value);
    if (draft !== state.draft) change(draft);
    cancelPause?.();
    cancelPause = undefined;
    if (commit) void validate();
    else if (draft !== state.draft)
      cancelPause = afterPause(() => {
        cancelPause = undefined;
        if (editor.value?.session === state.session) void validate();
      });
  }

  function structural(run: (draft: TemplateDraft) => { draft: TemplateDraft; part?: EditorPart } | TemplateDraft) {
    const state = editor.value;
    if (!state || state.draft.state === 'retired') return;
    const result = run(state.draft);
    if ('draft' in result) change(result.draft, result.part);
    else change(result);
    void validate();
  }

  const addTemplateInput = () => {
    structural(addInput);
  };
  const addTemplateStep = () => {
    structural(addStep);
  };
  const removePart = (part: EditorPart) => {
    structural((draft) =>
      part.startsWith('input:')
        ? removeInput(draft, part.slice('input:'.length))
        : part.startsWith('step:')
          ? removeStep(draft, part.slice('step:'.length))
          : draft,
    );
  };
  const move = (part: EditorPart, by: -1 | 1) => {
    structural((draft) => movePart(draft, part, by));
  };
  const addPrerequisite = (stepId: string) => {
    structural((draft) => addAfter(draft, stepId));
  };
  const removePrerequisite = (stepId: string, index: number) => {
    structural((draft) => removeAfter(draft, stepId, index));
  };

  /** Save the draft through `templates/set` with the revision it was opened from. */
  async function save() {
    const state = editor.value;
    if (!state || state.busy || state.draft.state === 'retired') return;
    putEditor({ busy: true, failure: undefined, conflict: false });
    const problems = await validate();
    if (editor.value?.session !== state.session) return;
    if (problems === undefined) {
      putEditor({ busy: false });
      return;
    }
    if (problems.length > 0) {
      const message =
        problems.length === 1 ? 'Fix 1 problem before saving.' : `Fix ${problems.length} problems before saving.`;
      putEditor({ busy: false, failure: message });
      dependencies.notify?.(message);
      return;
    }
    const draft = editor.value.draft,
      current = () => editor.value?.session === state.session;
    try {
      await call(state.projectId, 'set', {
        templates: [templateFromDraft(draft)],
        ...(state.revision ? { expected_revision: state.revision } : {}),
      });
      if (current()) editor.value = undefined;
      dependencies.notify?.(`Saved ${nameOf(draft)}.`);
      await load(state.projectId);
    } catch (reason) {
      if (!current()) return;
      const message = describeRefusal(reason, `Could not save ${nameOf(draft)}`);
      putEditor({ busy: false, failure: message, conflict: codeOf(reason) === 'template_conflict' });
      dependencies.notify?.(message);
      if (codeOf(reason) === 'template_invalid') void validate();
    }
  }

  /**
   * After a conflict: read the templates again. An existing template reopens as it now is (the draft's
   * changes are discarded, as the banner says); a new one keeps its draft against the new revision.
   */
  async function reload() {
    const state = editor.value;
    if (!state) return;
    await load(state.projectId);
    if (editor.value?.session !== state.session) return;
    const list = listOf(state.projectId);
    if (!state.draft.template_id) {
      putEditor({ revision: list.revision, conflict: false, failure: undefined });
      return;
    }
    const latest = list.templates.find((template) => template.template_id === state.draft.template_id);
    if (!latest) {
      putEditor({ conflict: false, failure: 'That template no longer exists.' });
      return;
    }
    const draft = draftFromTemplate(latest);
    putEditor({
      revision: list.revision,
      original: draft,
      draft,
      part: partExists(draft, state.part) ? state.part : 'template',
      problems: localProblems(draft),
      conflict: false,
      failure: undefined,
    });
  }

  /** Close the editor; unsaved changes are discarded only after asking. */
  async function close() {
    const state = editor.value;
    if (!state) return;
    if (
      state.draft.state !== 'retired' &&
      draftChanged(state.draft, state.original) &&
      !(await dependencies.confirm({
        title: 'Discard your changes?',
        message: `Your changes to ${nameOf(state.draft)} are not saved.`,
        confirmLabel: 'Discard Changes',
        tone: 'danger',
      }))
    )
      return;
    if (editor.value?.session === state.session) editor.value = undefined;
  }

  /** Retire a template: it can no longer be applied; work applied from it is unchanged. Asks first. */
  async function retire(projectId: string, templateId: string) {
    const template = listOf(projectId).templates.find((candidate) => candidate.template_id === templateId);
    if (!template || template.state === 'retired') return;
    const name = nameOf(template);
    if (
      !(await dependencies.confirm({
        title: `Retire ${name}?`,
        message:
          'It can no longer be applied, and its id is never reused. Goals already applied from it keep their work. This cannot be undone here.',
        confirmLabel: 'Retire Template',
        tone: 'danger',
      }))
    )
      return;
    putList(projectId, { busy: [...listOf(projectId).busy, templateId] });
    try {
      await call(projectId, 'retire', { template_id: templateId });
      if (editor.value?.projectId === projectId && editor.value.draft.template_id === templateId)
        editor.value = undefined;
      dependencies.notify?.(`Retired ${name}.`);
    } catch (reason) {
      dependencies.notify?.(describeRefusal(reason, `Could not retire ${name}`));
    } finally {
      putList(projectId, { busy: listOf(projectId).busy.filter((id) => id !== templateId) });
    }
    await load(projectId);
  }

  return {
    lists,
    editor,
    load,
    openNew,
    openEdit,
    select,
    back,
    edit,
    validate,
    addInput: addTemplateInput,
    addStep: addTemplateStep,
    removePart,
    move,
    addPrerequisite,
    removePrerequisite,
    save,
    reload,
    close,
    retire,
  };
}

export type OrchestrationTemplateEditing = ReturnType<typeof createOrchestrationTemplateEditing>;
