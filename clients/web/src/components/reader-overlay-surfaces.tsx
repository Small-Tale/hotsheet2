import type { SafeHtml } from 'kerfjs';

import type { CompatibilityAssessment } from '../compatibility';
import { AIConversation } from './ai-conversation';
import { AttachmentContextMenu } from './attachment-context-menu';
import { AttachmentGallery } from './attachment-gallery';
import { CommandRunDialog } from './command-run-dialog';
import { ConnectionDetailsDialog } from './connection-details-dialog';
import { type ConnectionState, ConnectionStateBanner } from './connection-state-banner';
import { NotWorkingDialog } from './not-working-dialog';
import { PermissionRequestPopup } from './permission-request-card';
import { AppTabContextMenu } from './project-tab-context-menu';
import { ChangeEvidenceDialog, RepositoryStatusPopover } from './repository-status-popover';
import { TicketCloseDialog } from './ticket-close-dialog';
import { TicketReader, type TicketReaderProps } from './ticket-reader';
import { TicketRowContextMenu } from './ticket-row-context-menu';

type AIConversationProps = Parameters<typeof AIConversation>[0];
type AttachmentContextMenuProps = Parameters<typeof AttachmentContextMenu>[0];
type AttachmentGalleryProps = Parameters<typeof AttachmentGallery>[0];
type CommandRunDialogProps = Parameters<typeof CommandRunDialog>[0];
type ConnectionDetailsDialogProps = Parameters<typeof ConnectionDetailsDialog>[0];
type NotWorkingDialogProps = Parameters<typeof NotWorkingDialog>[0];
type PermissionPopupProps = Parameters<typeof PermissionRequestPopup>[0];
type AppTabContextMenuProps = Parameters<typeof AppTabContextMenu>[0];
type ChangeEvidenceDialogProps = Parameters<typeof ChangeEvidenceDialog>[0];
type RepositoryStatusPopoverProps = Parameters<typeof RepositoryStatusPopover>[0];
type TicketCloseDialogProps = Parameters<typeof TicketCloseDialog>[0];
type TicketRowContextMenuProps = Parameters<typeof TicketRowContextMenu>[0];

export type ReaderLayerSurfaceProps = TicketReaderProps & {
  attachmentMenu?: AttachmentContextMenuProps;
  evidence?: ChangeEvidenceDialogProps;
};

export function ReaderLayerSurface({ attachmentMenu, evidence, ...reader }: ReaderLayerSurfaceProps) {
  return (
    <TicketReader
      {...reader}
      overlay={
        <>
          <AttachmentContextMenuSurface menu={attachmentMenu} />
          <ChangeEvidenceSurface evidence={evidence} />
        </>
      }
    />
  );
}
export function ReaderLayersSurface({ layers }: { layers: readonly SafeHtml[] }) {
  return <>{layers}</>;
}

export function CompatibilityBannerSurface({ assessment }: { assessment?: CompatibilityAssessment }) {
  if (!assessment || (assessment.kind === 'compatible' && !assessment.revisionMismatch)) return null;
  const state: ConnectionState =
    assessment.kind === 'compatible'
      ? 'revision-mismatch'
      : assessment.kind === 'unknown'
        ? 'compatibility-unknown'
        : (assessment.kind.replaceAll('_', '-') as ConnectionState);
  const recovery =
    assessment.kind === 'server_too_old' && !assessment.canRestartServer
      ? ' Safe restart is unavailable because this server cannot verify that active work will be preserved.'
      : '';
  const revision = assessment.sourceStale
    ? ' The running server was built from older local source. Rebuild if needed, then restart it to pick up your latest build.'
    : assessment.revisionMismatch
      ? ' The protocol is compatible, but this detached server was built from another revision.'
      : '';
  return <ConnectionStateBanner state={state} detail={`${assessment.detail ?? ''}${recovery}${revision}`.trim()} />;
}

export function ConnectionDetailsSurface(props: ConnectionDetailsDialogProps) {
  return <ConnectionDetailsDialog {...props} />;
}
export function PermissionPopupSurface({ popup }: { popup?: PermissionPopupProps }) {
  return popup ? <PermissionRequestPopup {...popup} /> : null;
}
export function AIConversationSurface({ conversation }: { conversation?: AIConversationProps }) {
  return conversation ? <AIConversation {...conversation} /> : null;
}
export function RepositoryStatusSurface({ repository }: { repository?: RepositoryStatusPopoverProps }) {
  return repository ? <RepositoryStatusPopover {...repository} /> : null;
}
export function ChangeEvidenceSurface({ evidence }: { evidence?: ChangeEvidenceDialogProps }) {
  return evidence ? <ChangeEvidenceDialog {...evidence} /> : null;
}

export interface TicketContextMenuSurfaceProps {
  menu?: TicketRowContextMenuProps;
  closeDialog: TicketCloseDialogProps['state'];
}
export function TicketContextMenuSurface({ menu, closeDialog }: TicketContextMenuSurfaceProps) {
  return (
    <>
      {menu && <TicketRowContextMenu {...menu} />}
      <TicketCloseDialog state={closeDialog} />
    </>
  );
}

export function CommandDialogSurface(props: CommandRunDialogProps) {
  return <CommandRunDialog {...props} />;
}
export function AttachmentContextMenuSurface({ menu }: { menu?: AttachmentContextMenuProps }) {
  return menu ? <AttachmentContextMenu {...menu} /> : null;
}
export function GallerySurface({
  gallery,
  menu,
}: {
  gallery?: AttachmentGalleryProps;
  menu?: AttachmentContextMenuProps;
}) {
  return gallery ? <AttachmentGallery {...gallery} overlay={<AttachmentContextMenuSurface menu={menu} />} /> : null;
}
export function AppTabMenuSurface({ menu }: { menu?: AppTabContextMenuProps }) {
  return menu ? <AppTabContextMenu {...menu} /> : <></>;
}
export function NotWorkingSurface(props: NotWorkingDialogProps) {
  return <NotWorkingDialog {...props} />;
}
