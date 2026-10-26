import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { List } from '@kerfjs/ui/list';
import { delegate, signal } from 'kerfjs';
import { action } from 'kerfjs/actions';
import { createScope } from 'kerfjs/scope';

import { createAiLaunchDialog } from '../ai-launch-dialog';
import { type AttachmentFilenameEditing, createAttachmentFilenameEditor } from '../attachment-filename-editing';
import { wireAttachmentFilenameEditor } from '../attachment-filename-interactions';
import { createCommandGroupDialog } from '../command-group-dialog';
import { AiLaunchDialog } from '../components/ai-launch-dialog';
import { AttachmentFilenameEditor } from '../components/attachment-filename-editor';
import { CommandGroupDialog } from '../components/command-group-dialog';
import { wireInputDialogs } from '../input-dialog-interactions';
import type { DemoModule } from './demo-module';

const openAi = action('demo-open-ai-launch');
const openGroup = action('demo-open-command-group');
const openFilename = action('demo-open-filename');

const ai = createAiLaunchDialog({
  tools: () => [{ id: 'fixture', display_name: 'Fixture AI', models: [] }],
  normalize: (selection) => ({ tool: selection.tool ?? 'fixture', model: selection.model, effort: selection.effort }),
});
const group = createCommandGroupDialog((_project, name) =>
  name.trim() === 'Existing' ? 'A group named "Existing" already exists.' : undefined,
);
const filenameState = signal<AttachmentFilenameEditing | undefined>(undefined);
const filename = createAttachmentFilenameEditor(filenameState, () => undefined);

export function AiLaunchDialogDemo() {
  return (
    <CatalogExample label="Interactive">
      <Button action={openAi.value}>New AI Chat</Button>
      <AiLaunchDialog state={ai.state.value} />
    </CatalogExample>
  );
}
export function CommandGroupDialogDemo() {
  return (
    <CatalogExample label="Interactive">
      <Button action={openGroup.value}>New Command Group</Button>
      <CommandGroupDialog state={group.state.value} />
    </CatalogExample>
  );
}
export function AttachmentFilenameEditorDemo() {
  return (
    <CatalogExample label="Interactive">
      <List gap="m">
        <Button action={openFilename.value}>Rename Attachment</Button>
        <AttachmentFilenameEditor name="proof.txt" edit={filenameState.value} />
      </List>
    </CatalogExample>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'ai-launch-dialog': {
      catalogExamples: true,
      Demo: AiLaunchDialogDemo,
      reset: () => {
        ai.cancel();
      },
    },
    'command-group-dialog': {
      catalogExamples: true,
      Demo: CommandGroupDialogDemo,
      reset: () => {
        group.cancel();
      },
    },
    'attachment-filename-editor': {
      catalogExamples: true,
      Demo: AttachmentFilenameEditorDemo,
      reset: () => {
        filename.cancel();
      },
    },
  },
  wire(root) {
    const scope = createScope();
    scope.add(wireInputDialogs(root, { ai, group }));
    scope.add(wireAttachmentFilenameEditor(root, filename));
    scope.add(
      delegate(root, 'click', openAi.selector, () => {
        void ai.request('demo', 'ai-chat', { tool: 'fixture', model: 'exact-model' });
      }),
    );
    scope.add(
      delegate(root, 'click', openGroup.selector, () => {
        group.open('demo');
      }),
    );
    scope.add(
      delegate(root, 'click', openFilename.selector, () => {
        filename.begin({ scope: 'demo', ticketKey: 'fixture:ticket', id: 'proof', original: 'proof.txt' }, 'demo', () =>
          Promise.resolve(),
        );
        queueMicrotask(() => root.querySelector<HTMLInputElement>('[name="attachment-filename"]')?.focus());
      }),
    );
    return () => {
      ai.cancel();
      group.cancel();
      filename.cancel();
      scope.dispose();
    };
  },
};
