import { signal } from 'kerfjs';

import { NotWorkingDialog } from '../components/not-working-dialog';
import { PendingAttachmentPicker } from '../components/pending-attachment-picker';

export const notWorkingDemoOpen=signal(false),notWorkingDemoNote=signal('The completed change still fails when the project is reopened.'),notWorkingDemoFiles=signal([{id:'diagnostic',name:'diagnostic screenshot with a deliberately long filename.png'}]),notWorkingDemoEvent=signal('');

// Keep the dialog mounted and drive its `open` property, exactly like the real app's
// NotWorkingSurface. wa-dialog opens modally on the open property transitioning false->true;
// conditionally rendering it already-open never fires that transition, so it stayed hidden
// (HS2-AVTV2D).
export function NotWorkingDialogDemo(){return <section class="metadata-control-demo" aria-label="NotWorkingDialog demo"><button type="button" data-action="open-not-working-demo">Open Not Working dialog</button><p role="status">{notWorkingDemoEvent.value}</p><NotWorkingDialog slug="HS2-DEMO" open={notWorkingDemoOpen.value} note={notWorkingDemoNote.value} attachments={notWorkingDemoFiles.value}/></section>}
export function PendingAttachmentPickerDemo(){return <section class="metadata-control-demo" aria-label="PendingAttachmentPicker demo"><PendingAttachmentPicker attachments={notWorkingDemoFiles.value}/></section>}
