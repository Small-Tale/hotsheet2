import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { NotWorkingDialog } from '../components/not-working-dialog';
import { PENDING_ATTACHMENT_PICKER, PendingAttachmentPicker } from '../components/pending-attachment-picker';
import { routeFilePicker } from '../file-picker-routes';
import { DemoEvent } from './components/demo-event';
import { DEMO_ACTIONS, DEMO_COMPONENTS, DEMO_FIELDS, DEMO_MARKERS } from './demo-actions';
import type { DemoModule } from './demo-module';

export const notWorkingDemoOpen = signal(false),
  notWorkingDemoNote = signal('The completed change still fails when the project is reopened.'),
  notWorkingDemoFiles = signal([
    { id: 'diagnostic', name: 'diagnostic screenshot with a deliberately long filename.png' },
  ]),
  notWorkingDemoEvent = signal('');

// Keep the dialog mounted and drive its `open` property, exactly like the real app's
// NotWorkingSurface. wa-dialog opens modally on the open property transitioning false->true;
// conditionally rendering it already-open never fires that transition, so it stayed hidden
// (HS2-AVTV2D).
export function NotWorkingDialogDemo() {
  return (
    <>
      <CatalogExample align="glyph" label="Interactive">
        <Button action={'open-not-working-demo'}>Open Not Working Dialog</Button>
        <NotWorkingDialog
          slug="HS2-DEMO"
          open={notWorkingDemoOpen.value}
          note={notWorkingDemoNote.value}
          attachments={notWorkingDemoFiles.value}
        />
      </CatalogExample>
      <DemoEvent>{notWorkingDemoEvent.value}</DemoEvent>
    </>
  );
}
export function PendingAttachmentPickerDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'medium' }}>
      <PendingAttachmentPicker attachments={notWorkingDemoFiles.value} />
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireNotWorkingDialogDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openNotWorkingDemo.selector, () => {
      notWorkingDemoOpen.value = true;
      notWorkingDemoEvent.value = '';
      queueMicrotask(() => root.querySelector<HTMLTextAreaElement>('[name="not-working-note"]')?.focus());
    }),
  );
  demoListeners.add(
    delegate(root, 'input', DEMO_FIELDS.notWorkingNote.selector, (_event, target) => {
      notWorkingDemoNote.value = (target as HTMLTextAreaElement).value;
    }),
  );
  demoListeners.add(
    routeFilePicker(root, PENDING_ATTACHMENT_PICKER, (files) => {
      notWorkingDemoFiles.value = [
        ...notWorkingDemoFiles.value,
        ...files.map((file, index) => ({
          id: `demo-${Date.now()}-${index}`,
          name: file.name,
        })),
      ];
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.removeNotWorkingAttachment.selector, (_event, target) => {
      notWorkingDemoFiles.value = notWorkingDemoFiles.value.filter(
        (item) => item.id !== (target as HTMLElement).dataset.pendingAttachmentId,
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'dragover', DEMO_MARKERS.notWorkingDropzone.selector, (event, target) => {
      event.preventDefault();
      (target as HTMLElement).dataset.dragging = 'true';
    }),
  );
  demoListeners.add(
    delegate(root, 'dragleave', DEMO_MARKERS.notWorkingDropzone.selector, (_event, target) => {
      delete (target as HTMLElement).dataset.dragging;
    }),
  );
  demoListeners.add(
    delegate(root, 'drop', DEMO_MARKERS.notWorkingDropzone.selector, (event, target) => {
      event.preventDefault();
      delete (target as HTMLElement).dataset.dragging;
      const files = (event as DragEvent).dataTransfer?.files;
      if (files?.length)
        notWorkingDemoFiles.value = [
          ...notWorkingDemoFiles.value,
          ...Array.from(files).map((file, index) => ({
            id: `drop-${Date.now()}-${index}`,
            name: file.name,
          })),
        ];
    }),
  );
  demoListeners.add(
    delegate(root, 'submit', DEMO_ACTIONS.submitNotWorking.selector, (event) => {
      event.preventDefault();
      notWorkingDemoEvent.value = 'Ticket returned to Not Started and added to Up Next.';
      notWorkingDemoOpen.value = false;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.cancelNotWorking.selector, () => {
      notWorkingDemoOpen.value = false;
      notWorkingDemoEvent.value = 'Report cancelled.';
    }),
  );
  demoListeners.add(
    delegate(root, 'wa-request-close', DEMO_COMPONENTS.notWorkingDialog.selector, (event) => {
      event.preventDefault();
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'not-working-dialog': { catalogExamples: true, Demo: NotWorkingDialogDemo },
    'pending-attachment-picker': { catalogExamples: true, Demo: PendingAttachmentPickerDemo },
  },
  wire: wireNotWorkingDialogDemo,
};
