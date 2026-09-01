import { signal } from 'kerfjs';

import { NotWorkingDialog } from '../components/not-working-dialog';
import { PendingAttachmentPicker } from '../components/pending-attachment-picker';

export const notWorkingDemoOpen=signal(false),notWorkingDemoNote=signal('The completed change still fails when the project is reopened.'),notWorkingDemoFiles=signal([{id:'diagnostic',name:'diagnostic screenshot with a deliberately long filename.png'}]),notWorkingDemoEvent=signal('');

export function NotWorkingDialogDemo(){return <section class="metadata-control-demo" aria-label="NotWorkingDialog demo"><button type="button" data-action="open-not-working-demo">Open Not Working dialog</button><p role="status">{notWorkingDemoEvent.value}</p>{notWorkingDemoOpen.value&&<NotWorkingDialog slug="HS2-DEMO" note={notWorkingDemoNote.value} attachments={notWorkingDemoFiles.value}/>}</section>}
export function PendingAttachmentPickerDemo(){return <section class="metadata-control-demo" aria-label="PendingAttachmentPicker demo"><PendingAttachmentPicker attachments={notWorkingDemoFiles.value}/></section>}
