import { CatalogExample } from '@kerfjs/ui/catalog';

import { OrchestrationSettings } from '../components/orchestration-settings';
import { wireOrchestrationSettings } from '../components/orchestration-settings-interactions';
import { OrchestrationTemplateEditor } from '../components/orchestration-template-editor';
import { wireOrchestrationTemplateEditor } from '../components/orchestration-template-editor-interactions';
import { templateSettingsRows } from '../components/orchestration-template-settings';
import type { TemplateEditorState } from '../features/orchestration-template-editing';
import { projectCollapseKey } from '../orchestration-project-tree';
import { addChoices, policyDraftFrom, projectRows, siteChoicesByProject } from '../orchestration-settings-model';
import {
  addInput,
  draftFromTemplate,
  editDraft,
  type EditorPart,
  newTemplateDraft,
} from '../orchestration-template-editor';
import type { DemoModule } from './demo-module';
import {
  createDemoOrchestrationSettings,
  createDemoTemplateEditing,
  DEMO_HOSTED_CHECKOUTS,
  DEMO_OPEN_PROJECTS,
  DEMO_OWNER,
  DEMO_REMOTE_LABELS,
  DEMO_SITE_ROWS,
  DEMO_TARGETS,
  demoCheckedInTemplates,
  demoToolLabel,
} from './orchestration-settings-fixtures';

const demo = createDemoOrchestrationSettings();
const templateDemo = createDemoTemplateEditing();

const DEMO_PROJECTS = [
  { target_id: DEMO_TARGETS.api, label: 'API' },
  { target_id: DEMO_TARGETS.web, label: 'Web' },
  { target_id: DEMO_TARGETS.docs, label: 'Docs' },
];
const DEMO_TOOLS = [
  { id: 'claude', label: 'Claude' },
  { id: 'codex', label: 'Codex' },
];

/** HS2-1XCW6N: one editor state over a checked-in demo template (or a new one), in place. */
function editorState(
  change: Partial<TemplateEditorState> = {},
  template: Record<string, unknown> | null = demoCheckedInTemplates()[0],
): TemplateEditorState {
  const draft = template === null ? newTemplateDraft(DEMO_TARGETS.api) : draftFromTemplate(template);
  return {
    projectId: DEMO_OWNER,
    revision: 'rev-1',
    original: draft,
    draft,
    part: 'template',
    detailActive: false,
    problems: [],
    validating: false,
    busy: false,
    conflict: false,
    session: 1,
    ...change,
  };
}

/** HS2-1XCW6N: the template editor's states, in place: each part, problems, a conflict, retired, phone. */
function TemplateEditorStates() {
  const base = draftFromTemplate(demoCheckedInTemplates()[0]),
    step: EditorPart = `step:${base.steps[0].id}`,
    input: EditorPart = `input:${base.inputs[0].id}`;
  const fresh = addInput(newTemplateDraft(DEMO_TARGETS.api)).draft;
  const examples: Array<[string, TemplateEditorState, boolean]> = [
    ['Template Editor: Template', editorState(), false],
    ['Template Editor: Goal', editorState({ part: 'goal' }), false],
    ['Template Editor: Input', editorState({ part: input }), false],
    ['Template Editor: Step', editorState({ part: step }), false],
    [
      'Template Editor: New Template With Problems',
      editorState(
        {
          original: fresh,
          draft: editDraft(fresh, 'goal.title', 'Rate limits: {{feature}}'),
          failure: 'Fix 3 problems before saving.',
          problems: [
            { code: 'name_invalid', at: 'name' },
            { code: 'input_label_invalid', at: 'inputs[0].label' },
            { code: 'text_invalid', at: 'steps[0].title' },
          ],
        },
        null,
      ),
      false,
    ],
    [
      'Template Editor: Step Problems',
      editorState({
        part: `step:${base.steps[1].id}`,
        draft: editDraft(base, `step.${base.steps[1].id}.title`, 'Show {{feature}} errors in {{nope}}'),
        problems: [
          { code: 'placeholder_unknown', at: 'steps[1].title' },
          { code: 'target_unknown', at: 'steps[1].target' },
          { code: 'step_gates_invalid', at: 'steps[1].after[0].gates' },
          { code: 'step_cycle', at: 'steps.web' },
        ],
      }),
      false,
    ],
    [
      'Template Editor: Changed Elsewhere',
      editorState({
        draft: editDraft(base, 'name', 'Feature across API and Web, renamed'),
        failure: 'Could not save Feature across API and Web, renamed: The templates changed meanwhile. Read it again.',
        conflict: true,
      }),
      false,
    ],
    [
      'Template Editor: Saving',
      editorState({ draft: editDraft(base, 'name', 'Renamed'), busy: true, validating: true }),
      false,
    ],
    ['Template Editor: Retired', editorState({}, demoCheckedInTemplates()[1]), false],
    ['Template Editor: Phone', editorState({ part: step, detailActive: true }), true],
  ];
  return (
    <>
      {examples.map(([label, state, compact]) => (
        <CatalogExample label={label} viewport={{ width: compact ? 'compact' : 'full' }}>
          <OrchestrationTemplateEditor
            state={state}
            projects={DEMO_PROJECTS}
            tools={DEMO_TOOLS}
            compact={compact}
            embedded
          />
        </CatalogExample>
      ))}
    </>
  );
}

/**
 * Settings → Orchestration (HS2-W27N1D): the production controller and handlers over an in-memory
 * orchestration project. Add Project adds an open project, a row's menu removes it, and Apply Policy
 * stores the limits.
 */
export function OrchestrationSettingsDemo() {
  const state = demo.settings.settings.value[DEMO_OWNER],
    labels = demo.settings.remoteLabels.value,
    rows = projectRows(state?.manifest, state?.bindings ?? {}, labels),
    choices = addChoices(DEMO_OPEN_PROJECTS, DEMO_OWNER, state?.bindings ?? {}, state?.manifest, labels);
  return (
    <>
      <CatalogExample label="Interactive" viewport={{ width: 'full' }}>
        {/* HS2-E9VVQF: API's Add a Site offers Mobile, a clone of the same ticket store. */}
        <OrchestrationSettings
          rows={rows}
          choices={choices}
          siteChoices={siteChoicesByProject(
            rows,
            choices,
            state?.stores ?? {},
            state?.manifest,
            state?.bindings ?? {},
            Boolean(state?.showAllSites),
          )}
          showAllSites={Boolean(state?.showAllSites)}
          openTabs={DEMO_OPEN_PROJECTS.map((project) => project.id)}
          selectedTarget={demo.settings.selected.value[DEMO_OWNER]}
          selectedNode={demo.settings.selectedNodes.value[DEMO_OWNER]}
          ownerId={DEMO_OWNER}
          collapsedGroups={demo.settings.collapsedGroups.value}
          loading={state?.loading ?? true}
          busy={state?.busy}
          message={state?.message}
          policy={{ draft: state?.draft, toolLabel: demoToolLabel }}
          templates={{
            rows: templateSettingsRows(templateDemo.editing.lists.value[DEMO_OWNER]?.templates ?? []),
            loading: templateDemo.editing.lists.value[DEMO_OWNER]?.loading ?? true,
            busy: templateDemo.editing.lists.value[DEMO_OWNER]?.busy,
          }}
        />
        {/* HS2-1XCW6N: New Template and a row open the production editor over the in-memory templates. */}
        <OrchestrationTemplateEditor
          state={templateDemo.editing.editor.value}
          projects={DEMO_PROJECTS}
          tools={DEMO_TOOLS}
        />
      </CatalogExample>
      {/* HS2-M1T6KN: projects with several sites as tree groups, home site first, beside a single-site project. */}
      <CatalogExample label="Several Sites" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          rows={DEMO_SITE_ROWS}
          ownerId="demo-sites"
          selectedTarget="demo-api-build"
          siteChoices={{ 'demo-api': [], 'demo-docs': [], 'demo-web': [] }}
          policy={{ draft: policyDraftFrom(undefined, ['claude']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
      <CatalogExample label="Several Sites, Collapsed" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          rows={DEMO_SITE_ROWS}
          ownerId="demo-sites"
          collapsedGroups={
            new Set([projectCollapseKey('demo-sites', 'demo-api'), projectCollapseKey('demo-sites', 'demo-web')])
          }
          busy={['demo-docs']}
          policy={{ draft: policyDraftFrom(undefined, ['claude']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
      <CatalogExample label="Templates: None Yet" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          templates={{ rows: [] }}
          policy={{ draft: policyDraftFrom(undefined, ['claude']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
      <CatalogExample label="Templates: Loading" viewport={{ width: 'full' }}>
        <OrchestrationSettings templates={{ loading: true }} />
      </CatalogExample>
      <CatalogExample label="Templates: Unreadable" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          templates={{
            rows: [],
            error:
              'Could not read the templates: The templates file is damaged. Restore it from Git, then read it again. (template_file_invalid)',
          }}
        />
      </CatalogExample>
      <CatalogExample label="Templates: Retiring" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          templates={{ rows: templateSettingsRows(demoCheckedInTemplates()), busy: ['01JTEMPLATE00000000000FEAT'] }}
        />
      </CatalogExample>
      <TemplateEditorStates />
      <CatalogExample label="No Projects Yet" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          choices={addChoices(DEMO_OPEN_PROJECTS, DEMO_OWNER, {}, undefined, DEMO_REMOTE_LABELS)}
          policy={{ draft: policyDraftFrom(undefined, ['claude', 'codex']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
      <CatalogExample label="Loading" viewport={{ width: 'full' }}>
        <OrchestrationSettings loading />
      </CatalogExample>
      <CatalogExample label="Open From Another Server" viewport={{ width: 'full' }}>
        {/* HS2-92FJXM: + Add Project offers the projects open on that server. HS2-DXAB4K: rows name that server. */}
        <OrchestrationSettings
          rows={projectRows(state?.manifest, state?.bindings ?? {}, labels, undefined, 'Build Host')}
          hostedChoices={DEMO_HOSTED_CHECKOUTS.map((choice, index) => ({
            checkoutId: choice.checkout_id,
            name: choice.name,
            added: index === 0,
          }))}
          hostedServer="Build Host"
          policy={{ draft: policyDraftFrom(undefined, ['claude']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
      <CatalogExample label="Another Server's Projects Unreadable" viewport={{ width: 'full' }}>
        <OrchestrationSettings
          hostedChoices={[]}
          hostedServer="Build Host"
          unavailableReason="Could not read the projects open on Build Host. The server did not answer."
          policy={{ draft: policyDraftFrom(undefined, ['claude']), toolLabel: demoToolLabel }}
        />
      </CatalogExample>
    </>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'orchestration-settings': {
      catalogExamples: true,
      Demo: OrchestrationSettingsDemo,
      reset: () => {
        void demo.reset();
        void templateDemo.reset();
      },
    },
  },
  wire: (root) => {
    void demo.reset();
    void templateDemo.reset();
    const settings = wireOrchestrationSettings(root, demo.settings, {
      projectId: () => DEMO_OWNER,
      activateProject: () => undefined,
    });
    const templates = wireOrchestrationTemplateEditor(root, templateDemo.editing, {
      projectId: () => DEMO_OWNER,
      firstProject: () => DEMO_TARGETS.api,
    });
    return () => {
      settings();
      templates();
    };
  },
};
