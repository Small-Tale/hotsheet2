import './attachment-gallery.css';

import { px } from '@kerfjs/ui/css-values';
import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import type { SafeHtml } from 'kerfjs';
import {
  ChevronLeft,
  ChevronRight,
  Crop,
  Eraser,
  Minus,
  MoreHorizontal,
  Pause,
  Pencil,
  Play,
  Plus,
  RotateCcw,
  Scan,
  Volume2,
  VolumeX,
  X,
} from 'lucide';

import { annotationDefaultIntent, annotationIntentColor } from '../annotation-intents';
import type { Attachment, MediaAnnotation } from '../api';
import { isVideoAttachment } from '../attachment-references';
import type { GalleryAnnotationTool } from '../gallery-annotation-editor';
import {
  ATTACHMENTS_AND_GALLERY_ACTIONS,
  ATTACHMENTS_AND_GALLERY_TARGETS,
} from '../interaction-attrs/attachments-and-gallery';
import { MarkdownPreview } from './markdown-preview';

export interface AttachmentGalleryImage {
  id: string;
  name: string;
  url: string;
  thumbnailUrl?: string;
  aliases?: readonly string[];
  ticket?: string;
  attachmentId?: string;
}
export interface AttachmentGallerySelection {
  url?: string;
  ticket?: string;
  name?: string;
  attachmentId?: string;
}
export interface AttachmentGalleryGeometry {
  naturalWidth: number;
  naturalHeight: number;
  availableWidth: number;
  availableHeight: number;
}
export interface AttachmentGalleryZoomModel {
  stops: number[];
  index: number;
  fitIndex: number;
  scale: number;
  canZoomOut: boolean;
  canZoomIn: boolean;
}
export type AttachmentGalleryKeyboardAction = { kind: 'toggle-playback' } | { kind: 'seek'; playheadMs: number };
export interface AttachmentGallerySwipeGesture {
  pointerId: number;
  startX: number;
  startY: number;
}
export interface AttachmentGallerySwipeStart {
  pointerId: number;
  clientX: number;
  clientY: number;
  button: number;
  markup: boolean;
  stage: boolean;
  interactive: boolean;
  horizontallyScrollable: boolean;
}
export interface AttachmentGalleryVideoResource {
  srcObject: MediaProvider | null;
  pause(): void;
  removeAttribute(name: string): void;
  load(): void;
}

const validDimension = (value: number) => Number.isFinite(value) && value > 0;
const sameScale = (left: number, right: number) => Math.abs(left - right) < 0.0001;

export function attachmentGalleryImageIndex(images: readonly AttachmentGalleryImage[], activeUrl: string): number {
  return images.findIndex((image) => image.url === activeUrl || image.aliases?.includes(activeUrl));
}

/** Resolves cyclic gallery navigation from the same immutable image projection used to render it. */
export function attachmentGalleryShiftUrl(
  images: readonly AttachmentGalleryImage[],
  activeUrl: string,
  delta: number,
): string | undefined {
  const index = attachmentGalleryImageIndex(images, activeUrl);
  return index < 0 || images.length === 0
    ? undefined
    : images[(index + (delta % images.length) + images.length) % images.length].url;
}

/** Resolves an opened thumbnail by durable identity before consulting potentially shared URL aliases. */
export function attachmentGallerySelectionUrl(
  images: readonly AttachmentGalleryImage[],
  selection: AttachmentGallerySelection,
): string | undefined {
  const byId = selection.attachmentId
    ? images.find((image) => image.attachmentId === selection.attachmentId)
    : undefined;
  if (byId) return byId.url;
  const byIdentity =
    selection.ticket && selection.name
      ? images.find((image) => image.ticket === selection.ticket && image.name === selection.name)
      : undefined;
  if (byIdentity) return byIdentity.url;
  const byUrl = selection.url ? images.find((image) => image.url === selection.url) : undefined;
  if (byUrl) return byUrl.url;
  const byAlias = selection.url ? images.find((image) => image.aliases?.includes(selection.url!)) : undefined;
  return byAlias?.url ?? selection.url;
}

export function attachmentGalleryZoomStops(geometry: AttachmentGalleryGeometry): { stops: number[]; fit: number } {
  const { naturalWidth, naturalHeight, availableWidth, availableHeight } = geometry;
  if (![naturalWidth, naturalHeight, availableWidth, availableHeight].every(validDimension))
    return { stops: [1], fit: 1 };
  const fit = Math.min(availableWidth / naturalWidth, availableHeight / naturalHeight);
  const cover = Math.max(availableWidth / naturalWidth, availableHeight / naturalHeight);
  const stops = [1, fit, cover]
    .sort((left, right) => left - right)
    .filter((value, index, all) => index === 0 || !sameScale(value, all[index - 1]));
  return { stops, fit };
}

export function attachmentGalleryZoomModel(
  geometry: AttachmentGalleryGeometry,
  selectedScale?: number,
): AttachmentGalleryZoomModel {
  const { stops, fit } = attachmentGalleryZoomStops(geometry);
  const target = validDimension(selectedScale ?? 0) ? selectedScale! : fit;
  let index = 0;
  for (let candidate = 1; candidate < stops.length; candidate++)
    if (Math.abs(stops[candidate] - target) < Math.abs(stops[index] - target)) index = candidate;
  const fitIndex = stops.findIndex((value) => sameScale(value, fit));
  return {
    stops,
    index,
    fitIndex: Math.max(0, fitIndex),
    scale: stops[index],
    canZoomOut: index > 0,
    canZoomIn: index < stops.length - 1,
  };
}

function GalleryButton({
  action,
  label,
  icon,
  disabled = false,
  className = '',
  count = 0,
}: {
  action: string;
  label: string;
  icon: typeof X;
  disabled?: boolean;
  className?: string;
  count?: number;
}) {
  const accessibleLabel = count > 0 ? `${label}, ${count} ${count === 1 ? 'annotation' : 'annotations'}` : label;
  return (
    <button
      type="button"
      class={className || undefined}
      data-action={action}
      aria-label={accessibleLabel}
      title={accessibleLabel}
      disabled={disabled}
    >
      <LucideIcon icon={icon} name={label.toLowerCase().replaceAll(' ', '-')} />
      {count > 0 && (
        <span class="attachment-gallery__annotation-count" aria-hidden="true">
          {count}
        </span>
      )}
    </button>
  );
}

/** The review context intentionally remains visible just outside a timed annotation. */
export const attachmentGalleryAnnotationTolerance = (durationMs: number) =>
  Math.max(1000, Math.max(0, durationMs) * 0.01);
export function attachmentGalleryAnnotationVisible(
  annotation: MediaAnnotation,
  playheadMs: number,
  durationMs: number,
) {
  if (annotation.start_ms === undefined) return true;
  const start = Math.min(annotation.start_ms, annotation.end_ms ?? annotation.start_ms),
    end = Math.max(annotation.start_ms, annotation.end_ms ?? annotation.start_ms),
    tolerance = attachmentGalleryAnnotationTolerance(durationMs);
  return playheadMs >= start - tolerance && playheadMs <= end + tolerance;
}
export function attachmentGalleryDefaultRange(playheadMs: number, durationMs: number) {
  const duration = Math.max(0, durationMs),
    playhead = Math.max(0, Math.min(duration, playheadMs)),
    radius = duration * 0.05;
  return {
    start_ms: Math.round(Math.max(0, playhead - radius)),
    end_ms: Math.round(Math.min(duration, playhead + radius)),
  };
}
export function attachmentGalleryKeyboardAction(
  key: string,
  playheadMs: number,
  durationMs: number,
  shiftKey = false,
): AttachmentGalleryKeyboardAction | undefined {
  const normalized = key.toLowerCase();
  if (key === ' ' || normalized === 'k') return { kind: 'toggle-playback' };
  const frame = 1000 / 30,
    coarse = 1000;
  let next: number | undefined;
  if (key === 'ArrowLeft') next = playheadMs - (shiftKey ? coarse : frame);
  else if (key === 'ArrowRight') next = playheadMs + (shiftKey ? coarse : frame);
  else if (normalized === 'j') next = playheadMs - coarse;
  else if (normalized === 'l') next = playheadMs + coarse;
  else if (key === 'Home') next = 0;
  else if (key === 'End') next = durationMs;
  return next === undefined
    ? undefined
    : { kind: 'seek', playheadMs: Math.round(Math.max(0, Math.min(Math.max(0, durationMs), next))) };
}
export function attachmentGallerySwipeGesture(
  start: AttachmentGallerySwipeStart,
): AttachmentGallerySwipeGesture | undefined {
  if (start.button !== 0 || start.markup || !start.stage || start.interactive || start.horizontallyScrollable)
    return undefined;
  return { pointerId: start.pointerId, startX: start.clientX, startY: start.clientY };
}
export function attachmentGallerySwipeDirection(
  gesture: AttachmentGallerySwipeGesture | undefined,
  pointerId: number,
  clientX: number,
  clientY: number,
): 1 | -1 | undefined {
  if (!gesture || gesture.pointerId !== pointerId) return undefined;
  const horizontal = clientX - gesture.startX,
    vertical = clientY - gesture.startY;
  if (Math.abs(horizontal) < 48 || Math.abs(horizontal) <= Math.abs(vertical)) return undefined;
  return horizontal < 0 ? 1 : -1;
}
/** Stops decoding and releases URL, stream, and buffered media resources before unmount. */
export function releaseAttachmentGalleryVideo(video: AttachmentGalleryVideoResource) {
  video.pause();
  video.removeAttribute('src');
  video.srcObject = null;
  video.load();
}
const annotationStyle = (annotation: MediaAnnotation) =>
  `left:${annotation.x / 100}%;top:${annotation.y / 100}%;width:${annotation.width / 100}%;height:${annotation.height / 100}%`;
const annotationShapeType = (annotation: MediaAnnotation) => annotation.shape?.type ?? 'rect';
const annotationPoint = (annotation: MediaAnnotation, point: { x: number; y: number }) =>
  `${((point.x - annotation.x) * 1000) / annotation.width},${((point.y - annotation.y) * 1000) / annotation.height}`;

/** Geometry stays in media coordinates; SVG keeps the stroke width in screen points. */
export function attachmentGalleryShapePath(annotation: MediaAnnotation): string {
  switch (annotation.shape?.type) {
    case undefined:
    case 'rect':
    case 'strike':
      return 'M 0,0 H 1000 V 1000 H 0 Z';
    case 'freehand':
      return `${annotation.shape.points.map((point, index) => `${index ? 'L' : 'M'} ${annotationPoint(annotation, point)}`).join(' ')}${annotation.shape.closed === false ? '' : ' Z'}`;
    case 'arrow':
      return annotation.shape.points
        .map((point, index) => `${index ? 'L' : 'M'} ${annotationPoint(annotation, point)}`)
        .join(' ');
    case 'insertion':
      return 'M 5,4 H 23 M 14,4 V 27 M 5,27 H 23 M 8,32 L 14,38 L 20,32';
  }
}

/** A badge is placed by its shape anchor and clamped to the media, including edge marks. */
export function attachmentGalleryBadgeStyle(annotation: MediaAnnotation): string {
  const anchor =
    annotation.shape?.type === 'arrow'
      ? annotation.shape.points[0]
      : annotation.shape?.type === 'insertion'
        ? annotation.shape.point
        : undefined;
  const x = (anchor?.x ?? annotation.x) / 100;
  const y = (anchor?.y ?? annotation.y) / 100;
  return `left:clamp(0px,calc(${x}% - 20px),calc(100% - 22px));top:clamp(0px,calc(${y}% - 20px),calc(100% - 22px))`;
}

/** Arrowhead dimensions are converted from screen points back into local media geometry. */
export function attachmentGalleryArrowHead(
  annotation: MediaAnnotation,
  mediaWidth: number,
  mediaHeight: number,
): string {
  if (annotation.shape?.type !== 'arrow' || annotation.shape.points.length < 2) return '';
  const points = annotation.shape.points,
    tail = points[points.length - 2],
    tip = points[points.length - 1],
    dx = ((tip.x - tail.x) * mediaWidth) / 10000,
    dy = ((tip.y - tail.y) * mediaHeight) / 10000,
    length = Math.hypot(dx, dy);
  if (!length) return '';
  const ux = dx / length,
    uy = dy / length,
    tipX = ((tip.x - annotation.x) * 1000) / annotation.width,
    tipY = ((tip.y - annotation.y) * 1000) / annotation.height,
    localX = (pixels: number) => (pixels * 10000 * 1000) / (mediaWidth * annotation.width),
    localY = (pixels: number) => (pixels * 10000 * 1000) / (mediaHeight * annotation.height),
    wing = (side: number) => `${tipX + localX(-12 * ux + side * 5 * uy)},${tipY + localY(-12 * uy - side * 5 * ux)}`;
  return `M ${tipX},${tipY} L ${wing(1)} L ${wing(-1)} Z`;
}

function AnnotationShape({
  annotation,
  mediaWidth,
  mediaHeight,
}: {
  annotation: MediaAnnotation;
  mediaWidth: number;
  mediaHeight: number;
}) {
  const type = annotationShapeType(annotation),
    arrowHead = attachmentGalleryArrowHead(annotation, mediaWidth, mediaHeight);
  return (
    <svg
      class="attachment-gallery__annotation-shape"
      viewBox={type === 'insertion' ? '0 0 28 40' : '0 0 1000 1000'}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <path class="attachment-gallery__annotation-halo" d={attachmentGalleryShapePath(annotation)} />
      <path
        class="attachment-gallery__annotation-ink"
        d={attachmentGalleryShapePath(annotation)}
        data-filled={String(
          type === 'rect' ||
            (type === 'freehand' && annotation.shape?.type === 'freehand' && annotation.shape.closed !== false),
        )}
      />
      {arrowHead && <path class="attachment-gallery__annotation-arrow-head" d={arrowHead} />}
      {type === 'strike' && (
        <>
          <path class="attachment-gallery__annotation-halo" d="M 0,0 L 1000,1000 M 1000,0 L 0,1000" />
          <path class="attachment-gallery__annotation-ink" d="M 0,0 L 1000,1000 M 1000,0 L 0,1000" />
        </>
      )}
    </svg>
  );
}
const formatTime = (milliseconds: number) => {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
};
const timelinePercent = (milliseconds: number | undefined, durationMs: number) =>
  durationMs > 0 ? Math.max(0, Math.min(100, ((milliseconds ?? 0) * 100) / durationMs)) : 0;

export function AttachmentGallery({
  images,
  activeUrl,
  geometry = { naturalWidth: 0, naturalHeight: 0, availableWidth: 0, availableHeight: 0 },
  selectedScale,
  annotations = [],
  imageUrl,
  cropMode = false,
  crop,
  cropEnabled = false,
  originalWidth = 0,
  originalHeight = 0,
  annotationNumberOffset = 0,
  markup = false,
  selectedAnnotation,
  drawMode = false,
  tool = drawMode ? 'rect' : 'select',
  playheadMs = 0,
  durationMs = 0,
  playing = false,
  volume = 1,
  muted = false,
  volumeOpen = false,
  annotationEnabled = true,
  overlay,
}: {
  images: readonly AttachmentGalleryImage[];
  activeUrl: string;
  geometry?: AttachmentGalleryGeometry;
  selectedScale?: number;
  annotations?: readonly MediaAnnotation[];
  imageUrl?: string;
  cropMode?: boolean;
  crop?: Attachment['crop'];
  cropEnabled?: boolean;
  originalWidth?: number;
  originalHeight?: number;
  annotationNumberOffset?: number;
  markup?: boolean;
  selectedAnnotation?: string;
  drawMode?: boolean;
  tool?: GalleryAnnotationTool;
  playheadMs?: number;
  durationMs?: number;
  playing?: boolean;
  volume?: number;
  muted?: boolean;
  volumeOpen?: boolean;
  annotationEnabled?: boolean;
  overlay?: SafeHtml;
}) {
  if (images.length === 0) return null;
  const index = Math.max(0, attachmentGalleryImageIndex(images, activeUrl)),
    image = images[index],
    video = isVideoAttachment(image.name),
    timed = video || durationMs > 0 || annotations.some((annotation) => annotation.start_ms !== undefined),
    noun = video ? 'video' : 'image',
    zoom = attachmentGalleryZoomModel(geometry, selectedScale);
  const selectedTimedAnnotation = markup
    ? annotations.find((annotation) => annotation.id === selectedAnnotation && annotation.start_ms !== undefined)
    : undefined;
  const selectedAnnotationNote = annotations.find(
    (annotation) =>
      annotation.id === selectedAnnotation && attachmentGalleryAnnotationVisible(annotation, playheadMs, durationMs),
  );
  const selectedEditableAnnotation = markup
    ? annotations.find((annotation) => annotation.id === selectedAnnotation)
    : undefined;
  const imageData = {
    'data-attachment-url': image.url,
    'data-attachment-name': image.name,
    'data-attachment-ticket': image.ticket,
    'data-gallery-attachment-id': image.attachmentId,
  };
  return (
    <dialog
      class="attachment-gallery"
      {...ATTACHMENTS_AND_GALLERY_TARGETS.attachmentGallery.attrs}
      aria-label={`${video ? 'Video' : 'Image'} ${index + 1} of ${images.length}: ${image.name}`}
    >
      <div class="attachment-gallery__toolbar">
        <Toolbar
          dividerSides=""
          // The full-screen gallery reaches the status bar and rounded corners: the toolbar claims
          // those device insets so its controls stay in the safe area (HS2-5TYNAS).
          safeAreaEdges={['block-start', 'inline-start', 'inline-end']}
          leading={<ToolbarText className="attachment-gallery__filename" text={image.name} tone="dark" fill />}
          trailing={
            <>
              <ToolbarControlGroup label="Media navigation" tone="dark">
                <GalleryButton
                  action="previous-gallery-image"
                  label={`Previous ${noun}`}
                  icon={ChevronLeft}
                  disabled={images.length < 2}
                />
                <span class="attachment-gallery__count">
                  {index + 1} / {images.length}
                </span>
                <GalleryButton
                  action="next-gallery-image"
                  label={`Next ${noun}`}
                  icon={ChevronRight}
                  disabled={images.length < 2}
                />
              </ToolbarControlGroup>
              <ToolbarControlGroup label="Media actions" tone="dark">
                <GalleryButton
                  action="toggle-gallery-markup"
                  label={markup ? 'Finish markup' : 'Annotate media'}
                  icon={Pencil}
                  disabled={!annotationEnabled}
                  className={markup ? 'attachment-gallery__pressed' : ''}
                  count={annotations.length}
                />
                <GalleryButton
                  action="open-gallery-attachment-menu"
                  label={`More ${noun} actions`}
                  icon={MoreHorizontal}
                />
              </ToolbarControlGroup>
              <ToolbarControlGroup label="Close gallery" tone="dark" single>
                <GalleryButton action="close-attachment-gallery" label={`Close ${noun} gallery`} icon={X} />
              </ToolbarControlGroup>
            </>
          }
        />
      </div>
      <div
        class="attachment-gallery__stage"
        data-gallery-zoom-stage="true"
        tabindex={video || markup ? '0' : undefined}
        role={markup ? 'group' : undefined}
        aria-label={
          video
            ? `Video canvas. Space or K plays and pauses; arrow keys step frames; Shift plus arrow, J, or L seeks one second.${markup ? ' V, R, F, A, I, and S choose annotation tools; Tab cycles marks; Enter edits the selected note; Escape cancels.' : ''}`
            : markup
              ? 'Image annotation canvas. V, R, F, A, I, and S choose tools; Tab cycles marks; Enter edits the selected note; Escape cancels.'
              : undefined
        }
      >
        <div class="attachment-gallery__canvas">
          <div
            class="attachment-gallery__media-wrap"
            {...ATTACHMENTS_AND_GALLERY_TARGETS.galleryAnnotationSurface.attrs}
            data-draw-mode={String(drawMode || cropMode)}
            {...(cropMode ? ATTACHMENTS_AND_GALLERY_TARGETS.galleryCropSurface.attrs : {})}
            style={
              validDimension(geometry.naturalWidth) && validDimension(geometry.naturalHeight)
                ? `width:${geometry.naturalWidth * zoom.scale}px;height:${geometry.naturalHeight * zoom.scale}px`
                : undefined
            }
          >
            {video ? (
              <video
                class="attachment-gallery__video"
                {...imageData}
                {...ATTACHMENTS_AND_GALLERY_TARGETS.galleryMedia.attrs}
                src={image.url}
                aria-label={image.name}
                playsInline
                preload="auto"
              />
            ) : (
              <img
                {...imageData}
                {...ATTACHMENTS_AND_GALLERY_TARGETS.galleryMedia.attrs}
                {...ATTACHMENTS_AND_GALLERY_TARGETS.galleryImage.attrs}
                src={imageUrl ?? image.url}
                alt={image.name}
              />
            )}{' '}
            {cropMode && crop && originalWidth > 0 && originalHeight > 0 && (
              <div
                class="attachment-gallery__crop-selection"
                style={`left:${(crop.x / originalWidth) * 100}%;top:${(crop.y / originalHeight) * 100}%;width:${(crop.width / originalWidth) * 100}%;height:${(crop.height / originalHeight) * 100}%`}
                aria-label="Crop selection"
              />
            )}
            {!cropMode && annotations.length > 0 && (
              <div class="attachment-gallery__annotations" data-markup={String(markup)}>
                {annotations.map((annotation, annotationIndex) => {
                  const number = annotationNumberOffset + annotationIndex + 1,
                    shape = annotationShapeType(annotation),
                    intents = annotation.intents?.length ? annotation.intents : [annotationDefaultIntent(annotation)],
                    visible = attachmentGalleryAnnotationVisible(annotation, playheadMs, durationMs),
                    color = annotationIntentColor(annotation);
                  return (
                    <>
                      <button
                        type="button"
                        disabled={!markup}
                        class="attachment-gallery__annotation"
                        {...ATTACHMENTS_AND_GALLERY_ACTIONS.selectGalleryAnnotation.attrs}
                        data-annotation-id={annotation.id}
                        data-annotation-start={annotation.start_ms}
                        data-annotation-end={annotation.end_ms}
                        data-selected={String(annotation.id === selectedAnnotation)}
                        data-shape={shape}
                        data-intent-color={color}
                        hidden={!visible}
                        style={annotationStyle(annotation)}
                        aria-label={`Annotation ${number}, ${shape}, ${intents.join(', ')}${annotation.text ? `: ${annotation.text}` : ''}`}
                      >
                        <AnnotationShape
                          annotation={annotation}
                          mediaWidth={geometry.naturalWidth * zoom.scale || geometry.availableWidth || 1000}
                          mediaHeight={geometry.naturalHeight * zoom.scale || geometry.availableHeight || 1000}
                        />
                        <span
                          class="attachment-gallery__annotation-label"
                          {...ATTACHMENTS_AND_GALLERY_ACTIONS.editGalleryAnnotation.attrs}
                          data-annotation-id={annotation.id}
                          aria-hidden="true"
                        >
                          {annotation.text}
                        </span>
                        {markup &&
                          annotation.id === selectedAnnotation &&
                          shape === 'arrow' &&
                          annotation.shape?.type === 'arrow' &&
                          annotation.shape.points.map((point, index) => (
                            <i
                              data-annotation-handle={`point-${index}`}
                              style={`left:${((point.x - annotation.x) / annotation.width) * 100}%;top:${((point.y - annotation.y) / annotation.height) * 100}%`}
                            />
                          ))}
                        {markup &&
                          annotation.id === selectedAnnotation &&
                          shape !== 'insertion' &&
                          shape !== 'arrow' &&
                          ((annotation.width *
                            (geometry.naturalWidth * zoom.scale || geometry.availableWidth || 1000)) /
                            10_000 <
                            36 ||
                          (annotation.height *
                            (geometry.naturalHeight * zoom.scale || geometry.availableHeight || 1000)) /
                            10_000 <
                            36
                            ? ['nw', 'ne', 'se', 'sw']
                            : ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w']
                          ).map((handle) => <i data-annotation-handle={handle} />)}
                      </button>
                      <span
                        class="attachment-gallery__annotation-badge"
                        data-intent-color={color}
                        data-annotation-id={annotation.id}
                        style={attachmentGalleryBadgeStyle(annotation)}
                        hidden={!visible}
                        aria-hidden="true"
                      >
                        {number}
                      </span>
                    </>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>
      <footer class="attachment-gallery__footer">
        {selectedEditableAnnotation && (
          <section class="attachment-gallery__editor" aria-label="Selected annotation editor">
            <label>
              Note (Markdown)
              <textarea
                {...ATTACHMENTS_AND_GALLERY_ACTIONS.editGalleryNote.attrs}
                aria-label="Annotation note"
                rows={2}
              >
                {selectedEditableAnnotation.text}
              </textarea>
            </label>
            <div class="attachment-gallery__intent-list" role="group" aria-label="Annotation intents">
              {(['comment', 'bug', 'change', 'insert', 'remove', 'move', 'question'] as const).map((intent) => {
                const fallback = annotationDefaultIntent(selectedEditableAnnotation),
                  chosen = selectedEditableAnnotation.intents?.length ? selectedEditableAnnotation.intents : [fallback];
                return (
                  <button
                    type="button"
                    {...ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryIntent.attrs}
                    data-intent={intent}
                    aria-pressed={String(chosen.includes(intent))}
                    title={
                      intent === fallback && !selectedEditableAnnotation.intents?.length ? 'Default intent' : undefined
                    }
                  >
                    {intent}
                    {intent === fallback && !selectedEditableAnnotation.intents?.length ? ' (default)' : ''}
                  </button>
                );
              })}
            </div>
            {selectedEditableAnnotation.shape?.type === 'freehand' && (
              <label class="attachment-gallery__closed-toggle">
                <input
                  type="checkbox"
                  {...ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryClosed.attrs}
                  checked={selectedEditableAnnotation.shape.closed !== false}
                />
                Closed outline
              </label>
            )}
          </section>
        )}
        {selectedAnnotationNote?.text && (
          <section
            class="attachment-gallery__selected-note"
            aria-label={`Annotation ${annotationNumberOffset + annotations.indexOf(selectedAnnotationNote) + 1} note`}
          >
            <MarkdownPreview source={selectedAnnotationNote.text} tone="inverse" size="small" density="compact" />
          </section>
        )}
        {timed && (
          <div class="attachment-gallery__timeline">
            <button
              class="attachment-gallery__playback"
              type="button"
              {...ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryPlayback.attrs}
              aria-label={playing ? 'Pause' : 'Play'}
            >
              <LucideIcon icon={playing ? Pause : Play} name={playing ? 'pause' : 'play'} size={24} />
            </button>
            <span data-gallery-current-time="true">{formatTime(playheadMs)}</span>
            <div class="attachment-gallery__timeline-track">
              {annotations
                .filter((annotation) => annotation.start_ms !== undefined)
                .map((annotation) => {
                  const start = timelinePercent(annotation.start_ms, durationMs),
                    end = timelinePercent(annotation.end_ms ?? annotation.start_ms, durationMs),
                    hasRange = end > start;
                  return (
                    <button
                      type="button"
                      class="attachment-gallery__timeline-annotation"
                      {...ATTACHMENTS_AND_GALLERY_ACTIONS.seekGalleryAnnotation.attrs}
                      data-annotation-id={annotation.id}
                      data-annotation-time={annotation.start_ms}
                      data-selected={String(annotation.id === selectedAnnotation)}
                      data-has-range={String(hasRange)}
                      style={`--annotation-start:${start}%;--annotation-end:${end}%`}
                      aria-label={`Annotation ${annotationNumberOffset + annotations.indexOf(annotation) + 1} at ${formatTime(annotation.start_ms ?? 0)}${annotation.text ? `: ${annotation.text}` : ''}`}
                    />
                  );
                })}
              <input
                type="range"
                name="gallery-playhead"
                min="0"
                max={String(Math.max(1, durationMs))}
                value={String(playheadMs)}
                aria-label="Video position"
              />
              {selectedTimedAnnotation && (
                <>
                  <button
                    type="button"
                    class="attachment-gallery__range-handle attachment-gallery__range-handle--start"
                    data-gallery-range-handle="start"
                    data-annotation-id={selectedTimedAnnotation.id}
                    style={`left:${timelinePercent(selectedTimedAnnotation.start_ms, durationMs)}%`}
                    aria-label={`Annotation range start at ${formatTime(selectedTimedAnnotation.start_ms ?? 0)}`}
                  >
                    [
                  </button>
                  <button
                    type="button"
                    class="attachment-gallery__range-handle attachment-gallery__range-handle--end"
                    data-gallery-range-handle="end"
                    data-annotation-id={selectedTimedAnnotation.id}
                    style={`left:${timelinePercent(selectedTimedAnnotation.end_ms ?? selectedTimedAnnotation.start_ms, durationMs)}%`}
                    aria-label={`Annotation range end at ${formatTime(selectedTimedAnnotation.end_ms ?? selectedTimedAnnotation.start_ms ?? 0)}`}
                  >
                    ]
                  </button>
                </>
              )}
            </div>
            <span>{formatTime(durationMs)}</span>
            {video && (
              <div class="attachment-gallery__volume">
                <button
                  type="button"
                  {...ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryVolume.attrs}
                  aria-label="Volume controls"
                  aria-expanded={String(volumeOpen)}
                >
                  <LucideIcon
                    icon={muted || volume === 0 ? VolumeX : Volume2}
                    name={muted || volume === 0 ? 'volume-x' : 'volume-2'}
                    size={24}
                  />
                </button>
                <div
                  class="attachment-gallery__volume-popup"
                  role="group"
                  aria-label="Volume controls"
                  hidden={!volumeOpen}
                >
                  <span>Volume</span>
                  <input
                    type="range"
                    name="gallery-volume"
                    min="0"
                    max="1"
                    step="0.05"
                    value={String(volume)}
                    aria-label="Video volume"
                  />
                  <button
                    type="button"
                    {...ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryMuted.attrs}
                    aria-label={muted || volume === 0 ? 'Unmute video' : 'Mute video'}
                  >
                    <LucideIcon
                      icon={muted || volume === 0 ? VolumeX : Volume2}
                      name={muted || volume === 0 ? 'volume-x' : 'volume-2'}
                      size={16}
                    />
                    <span>{muted || volume === 0 ? 'Unmute' : 'Mute'}</span>
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
        <div
          class="attachment-gallery__footer-actions"
          data-markup={String(markup)}
          data-has-crop={String(markup && cropEnabled && !video)}
          data-crop-mode={String(cropMode)}
        >
          {markup && cropEnabled && !video && (
            <span class="attachment-gallery__crop-actions">
              <button
                type="button"
                {...ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryCrop.attrs}
                aria-label={cropMode ? 'Finish crop' : 'Crop image'}
                aria-pressed={String(cropMode)}
                title={cropMode ? 'Finish crop' : 'Crop image'}
                class={cropMode ? 'attachment-gallery__pressed' : undefined}
              >
                <LucideIcon icon={Crop} name="crop" />
                {cropMode ? 'Finish crop' : 'Crop'}
              </button>
              {crop && (
                <button
                  type="button"
                  {...ATTACHMENTS_AND_GALLERY_ACTIONS.restoreGalleryCrop.attrs}
                  aria-label="Restore full image"
                  title="Restore full image"
                >
                  <LucideIcon icon={RotateCcw} name="rotate-ccw" />
                  Restore original
                </button>
              )}
              {cropMode && (
                <span class="attachment-gallery__crop-hint">Drag on the image to choose the visible area.</span>
              )}
            </span>
          )}
          {markup && !cropMode && (
            <span class="attachment-gallery__markup-position">
              <FloatingToolbar label="Media markup" position="bottom" inset={px(0)}>
                <ToolbarControlGroup label="Media markup">
                  <GalleryButton
                    action="toggle-gallery-draw"
                    label="Add rectangle"
                    icon={Scan}
                    className={tool === 'rect' ? 'attachment-gallery__pressed' : ''}
                  />
                  {(['select', 'freehand', 'arrow', 'insertion', 'strike'] as const).map((choice) => (
                    <button
                      type="button"
                      {...ATTACHMENTS_AND_GALLERY_ACTIONS.selectGalleryTool.attrs}
                      data-tool={choice}
                      aria-label={`${choice} tool`}
                      aria-pressed={String(tool === choice)}
                      title={`${choice} tool`}
                      class={tool === choice ? 'attachment-gallery__pressed' : undefined}
                    >
                      {({ select: 'V', freehand: 'F', arrow: 'A', insertion: 'I', strike: 'S' } as const)[choice]}
                    </button>
                  ))}
                  <GalleryButton
                    action="delete-gallery-annotation"
                    label="Erase selected annotation"
                    icon={Eraser}
                    disabled={!selectedAnnotation}
                  />
                </ToolbarControlGroup>
              </FloatingToolbar>
            </span>
          )}
          <FloatingToolbar label="Media zoom" position="bottom-end" inset={px(0)}>
            <ToolbarControlGroup label="Media zoom">
              <button
                type="button"
                {...ATTACHMENTS_AND_GALLERY_ACTIONS.zoomGalleryImage.attrs}
                data-zoom-direction="out"
                aria-label="Zoom out"
                title="Zoom out"
                disabled={!zoom.canZoomOut}
              >
                <LucideIcon icon={Minus} name="minus" />
              </button>
              <button
                type="button"
                {...ATTACHMENTS_AND_GALLERY_ACTIONS.zoomGalleryImage.attrs}
                data-zoom-direction="in"
                aria-label="Zoom in"
                title="Zoom in"
                disabled={!zoom.canZoomIn}
              >
                <LucideIcon icon={Plus} name="plus" />
              </button>
            </ToolbarControlGroup>
          </FloatingToolbar>
        </div>
      </footer>
      {overlay}
    </dialog>
  );
}
