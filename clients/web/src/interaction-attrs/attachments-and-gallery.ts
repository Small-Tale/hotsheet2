import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: attachments, the media gallery, and annotations.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/attachments-and-gallery.ts` register `.selector`.
 */
export const ATTACHMENTS_AND_GALLERY_ACTIONS = {
  editAttachmentBatchLabel: action('edit-attachment-batch-label'),
  openAttachmentRow: action('open-attachment-row'),
  openAttachmentGallery: action('open-attachment-gallery'),
  closeAttachmentGallery: action('close-attachment-gallery'),
  previousGalleryImage: action('previous-gallery-image'),
  nextGalleryImage: action('next-gallery-image'),
  zoomGalleryImage: action('zoom-gallery-image'),
  openGalleryAttachmentMenu: action('open-gallery-attachment-menu'),
  openAttachmentMenu: action('open-attachment-menu'),
  openReferencedAttachment: action('open-referenced-attachment'),
  attachmentMenuAction: action('attachment-menu-action'),
  toggleGalleryMarkup: action('toggle-gallery-markup'),
  toggleGalleryDraw: action('toggle-gallery-draw'),
  selectGalleryAnnotation: action('select-gallery-annotation'),
  editGalleryAnnotation: action('edit-gallery-annotation'),
  deleteGalleryAnnotation: action('delete-gallery-annotation'),
  seekGalleryAnnotation: action('seek-gallery-annotation'),
  toggleGalleryPlayback: action('toggle-gallery-playback'),
  toggleGalleryVolume: action('toggle-gallery-volume'),
  toggleGalleryMuted: action('toggle-gallery-muted'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * attachments, the media gallery, and annotations.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const ATTACHMENTS_AND_GALLERY_TARGETS = {
  attachmentBatchLabelField: attr('name', 'attachment-batch-label'),
  attachmentBatchPurposeField: attr('name', 'attachment-batch-purpose'),
  galleryImage: attr('data-gallery-image', 'true'),
  galleryMedia: attr('data-gallery-media', 'true'),
  galleryAnnotationSurface: attr('data-gallery-annotation-surface', 'true'),
  attachmentGallery: attr('data-component', 'attachment-gallery'),
} as const;
