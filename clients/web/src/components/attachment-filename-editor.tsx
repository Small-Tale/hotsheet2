import './attachment-filename-editor.css';

import { List } from '@kerfjs/ui/list';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { TextField } from '@kerfjs/ui/text-field';

import type { AttachmentFilenameEditing } from '../attachment-filename-editing';
import { INPUT_DIALOG_TARGETS as targets } from '../interaction-attrs/input-dialogs';

export function AttachmentFilenameEditor({ edit, name }: { edit?: AttachmentFilenameEditing; name: string }) {
  return (
    <div class="attachment-filename-editor" hidden={!edit} data-attachment-filename-editor="true">
      <List gap="xs">
        <TextField
          {...targets.filename.attrs}
          ariaLabel={`Filename for ${name}`}
          value={edit?.draft ?? name}
          size="compact"
          disabled={edit?.saving}
        />
        {edit?.message ? (
          <StateBanner tone="danger" title="Filename Could Not Be Changed" detail={edit.message} />
        ) : null}
      </List>
    </div>
  );
}
