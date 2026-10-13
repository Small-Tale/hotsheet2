import { CatalogExample } from '@kerfjs/ui/catalog';
import { delegate, delegateCapture, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { MediaAnnotation, StartedPhase } from '../api';
import {
  type AttachmentLabelEditing,
  attachmentLabelEditingDraft,
  attachmentLabelEditingKey,
  createAttachmentLabelEditor,
  INSPECTOR_ATTACHMENT_LABEL_SCOPE,
} from '../attachment-label-editing';
import { browserRandomId } from '../browser-id';
import { AttachmentContextMenu } from '../components/attachment-context-menu';
import { AttachmentGallery, attachmentGalleryZoomModel } from '../components/attachment-gallery';
import { wireAttachmentGalleryControls } from '../components/attachment-gallery-interactions';
import { type TicketAttachmentItem, TicketAttachments } from '../components/ticket-attachments';
import { wireTicketAttachmentActions } from '../components/ticket-attachments-interactions';
import { TicketCategorySelect } from '../components/ticket-category-select';
import { TicketCodeReview } from '../components/ticket-code-review';
import { TicketInfoPanel } from '../components/ticket-info-panel';
import { TicketPrioritySelect } from '../components/ticket-priority-select';
import { TicketStatusMenu } from '../components/ticket-status-menu';
import { TicketTimeline } from '../components/ticket-timeline';
import { ticketDraftKey } from '../ticket-draft-store';
import { selectedId } from './components/demo-state';
import { readerAttachments } from './content-components-demo';
import { DEMO_ACTIONS, DEMO_MARKERS } from './demo-actions';
import type { DemoModule } from './demo-module';
import { listen } from './demo-module';
import { recordCollectionEvent } from './ticket-collections-demo';

export function TicketCategorySelectDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'control' }}>
      <TicketCategorySelect name="demo-category" value="feature" />
    </CatalogExample>
  );
}
export function TicketPrioritySelectDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'control' }}>
      <TicketPrioritySelect name="demo-priority" value="urgent" />
    </CatalogExample>
  );
}
export function TicketStatusMenuDemo() {
  return (
    <>
      <CatalogExample align="glyph" label="Status" rootAttributes={{ 'data-status-menu-demo': '' }}>
        <TicketStatusMenu
          value={statusMenuDemoStatus.value}
          startedPhase={statusMenuDemoStartedPhase.value || undefined}
          canEditStartedPhase
        />
      </CatalogExample>
      <CatalogExample align="glyph" label="Read-only provider">
        <TicketStatusMenu value="started" startedPhase="final_testing" disabled />
      </CatalogExample>
    </>
  );
}
export const statusMenuDemoStatus = signal<
  'not_started' | 'started' | 'completed' | 'verified' | 'backlog' | 'archive'
>('started');
export const statusMenuDemoStartedPhase = signal<StartedPhase | ''>('planning');
export function TicketInfoPanelDemo() {
  return (
    <>
      <CatalogExample
        label="Loaded"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketInfoPanel
          status="started"
          priority="high"
          category="feature"
          tags={['client', 'ux']}
          details={
            '## Implementation notes\n\nBuild the reusable metadata and details presentation independently from the inspector shell.'
          }
          blockedReason="Waiting for final design review."
          notes={[
            {
              id: 'review',
              kind: 'regular',
              author: 'Claude',
              time: '10 minutes ago',
              body: 'The metadata and notes now share the inspector’s controlled state.',
            },
          ]}
          providerName="Hot Sheet git"
          updatedLabel="Updated 2 minutes ago"
        />
      </CatalogExample>
      <CatalogExample
        label="Placeholder"
        note="The value-free loading state the inspector skeleton composes."
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketInfoPanel placeholder />
      </CatalogExample>
    </>
  );
}
export function TicketTimelineDemo() {
  return (
    <CatalogExample
      label="Interactive"
      viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
    >
      <TicketTimeline />
    </CatalogExample>
  );
}
const demoImages = [
  {
    id: 'wide',
    name: 'wide-layout.svg',
    url: '/ux-gallery-preview.svg',
    annotationCount: 2,
    round: 1,
    batch_id: 'problem',
    actor: { display_name: 'Brian', role: 'human' as const },
    purpose: 'problem_evidence' as const,
  },
  {
    id: 'narrow',
    name: 'narrow-layout.svg',
    url: '/ux-gallery-preview.svg?variant=narrow',
    round: 1,
    batch_id: 'problem',
    actor: { display_name: 'Brian', role: 'human' as const },
    purpose: 'problem_evidence' as const,
  },
  {
    id: 'video',
    name: 'walkthrough.mp4',
    url: '/ux-gallery-video.mp4',
    thumbnailUrl: '/ux-gallery-preview.svg?variant=video',
    annotationCount: 1,
    batch_id: 'fix',
    batch_label: 'Corrected implementation',
    actor: { identity: 'codex', role: 'ai' as const },
    purpose: 'correctness_evidence' as const,
  },
];
export const galleryDemoUrl = signal<string | undefined>(demoImages[0].url);
export const galleryDemoScale = signal<number | undefined>(undefined);
export const galleryDemoMarkup = signal(false),
  galleryDemoDrawMode = signal(false),
  galleryDemoSelectedAnnotation = signal<string | undefined>('demo-annotation'),
  galleryDemoPlayhead = signal(1500),
  galleryDemoPlaying = signal(false),
  galleryDemoVolume = signal(1),
  galleryDemoMuted = signal(false),
  galleryDemoVolumeOpen = signal(false);
const imageAnnotations = [
  { id: 'demo-annotation', x: 900, y: 1100, width: 3000, height: 2400, text: 'Review this alignment' },
];
const shapeShowcaseAnnotations: MediaAnnotation[] = [
  { id: 'comment', x: 900, y: 1000, width: 1800, height: 1700, text: 'General **comment**' },
  { id: 'bug', x: 3300, y: 1000, width: 1800, height: 1700, text: 'Broken', intents: ['comment', 'bug'] },
  {
    id: 'change',
    x: 5700,
    y: 1000,
    width: 2200,
    height: 1700,
    text: 'Change this',
    shape: {
      type: 'freehand',
      points: [
        { x: 5700, y: 1000 },
        { x: 7900, y: 1200 },
        { x: 7300, y: 2700 },
      ],
    },
    intents: ['comment', 'change'],
  },
  {
    id: 'insert',
    x: 1900,
    y: 5000,
    width: 1,
    height: 1,
    text: 'Add **detail**',
    shape: { type: 'insertion', point: { x: 1900, y: 5000 } },
  },
  { id: 'remove', x: 3300, y: 4600, width: 1800, height: 1700, text: 'Remove this', shape: { type: 'strike' } },
  {
    id: 'move',
    x: 5800,
    y: 4600,
    width: 2500,
    height: 1700,
    text: 'Move here',
    shape: {
      type: 'arrow',
      points: [
        { x: 5800, y: 6300 },
        { x: 8300, y: 4600 },
      ],
    },
  },
  { id: 'question', x: 900, y: 7600, width: 1800, height: 1500, text: 'Why?', intents: ['comment', 'question'] },
];
export const galleryDemoVideoAnnotations = signal([
  {
    id: 'demo-annotation',
    x: 1200,
    y: 1800,
    width: 2800,
    height: 2600,
    start_ms: 1000,
    end_ms: 3000,
    text: 'Transition is abrupt',
  },
]);
function galleryDemoGeometry() {
  const width = typeof window === 'undefined' ? 1140 : window.innerWidth,
    height = typeof window === 'undefined' ? 820 : window.innerHeight;
  return {
    naturalWidth: 1600,
    naturalHeight: 1000,
    availableWidth: Math.max(320, width - 40),
    availableHeight: Math.max(240, height - 220),
  };
}
export function setGalleryDemo(open: boolean) {
  galleryDemoUrl.value = open ? demoImages[0].url : undefined;
  galleryDemoScale.value = undefined;
  galleryDemoMarkup.value = false;
  galleryDemoDrawMode.value = false;
  galleryDemoSelectedAnnotation.value = 'demo-annotation';
  galleryDemoPlayhead.value = 1500;
  galleryDemoPlaying.value = false;
  galleryDemoVolume.value = 1;
  galleryDemoMuted.value = false;
  galleryDemoVolumeOpen.value = false;
}
export function shiftGalleryDemo(delta: number) {
  const index = demoImages.findIndex((image) => image.url === galleryDemoUrl.value);
  galleryDemoUrl.value = demoImages[(Math.max(0, index) + delta + demoImages.length) % demoImages.length].url;
  galleryDemoScale.value = undefined;
  galleryDemoPlayhead.value = 1500;
}
export function zoomGalleryDemo(direction: 'in' | 'out') {
  const model = attachmentGalleryZoomModel(galleryDemoGeometry(), galleryDemoScale.value);
  galleryDemoScale.value = model.stops[model.index + (direction === 'in' ? 1 : -1)];
}
export function setGalleryDemoAnnotationEndpoint(endpoint: 'start' | 'end', milliseconds: number) {
  galleryDemoVideoAnnotations.value = galleryDemoVideoAnnotations.value.map((annotation) => {
    const next = Math.max(0, Math.min(6000, Math.round(milliseconds)));
    return endpoint === 'start'
      ? { ...annotation, start_ms: Math.min(next, annotation.end_ms) }
      : { ...annotation, end_ms: Math.max(annotation.start_ms, next) };
  });
}
export const attachmentDemoMenu = signal<{ x: number; y: number } | undefined>(undefined);
/** The demo's open batch label editor, owned by state like production's (HS2-SG0AZY). */
export const attachmentDemoLabelEditing = signal<AttachmentLabelEditing | undefined>(undefined);
const attachmentDemoItems = signal<TicketAttachmentItem[]>([
  ...demoImages,
  {
    id: 'diagnostics',
    name: 'hotsheet-ui-diagnostics.json',
    batch_id: 'automatic-diagnostics',
    actor: { identity: 'ui-stability-diagnostics', role: 'system' as const },
    purpose: 'problem_evidence' as const,
  },
  {
    id: 'demo-video',
    name: 'choppy.mov',
    url: '/ux-gallery-video.mov',
    thumbnailUrl: '/ux-gallery-preview.svg?variant=video',
  },
]);
export function showAttachmentDemoMenu(x: number, y: number) {
  attachmentDemoMenu.value = { x, y };
}
export function closeAttachmentDemoMenu() {
  attachmentDemoMenu.value = undefined;
}
export function regroupAttachmentDemo(id: string, destinationBatch?: string) {
  const destination = destinationBatch
    ? attachmentDemoItems.value.find((item) => item.batch_id === destinationBatch)
    : undefined;
  attachmentDemoItems.value = attachmentDemoItems.value.map((item) =>
    item.id === id
      ? {
          ...item,
          batch_id: destinationBatch ?? browserRandomId(),
          batch_label: destination ? destination.batch_label : 'New group',
          actor: destination?.actor ?? item.actor,
          purpose: destination?.purpose,
        }
      : item,
  );
}
export function renameAttachmentDemoBatch(batchId: string, label: string) {
  const nextBatchId = batchId || browserRandomId();
  attachmentDemoItems.value = attachmentDemoItems.value.map((item) =>
    (item.batch_id ?? '') === batchId
      ? { ...item, batch_id: nextBatchId, batch_label: label.trim() || undefined }
      : item,
  );
}
/**
 * The demo composes the production label editor (HS2-0QQHSZ) over its fixture: the same controlled
 * draft, 150 ms recovery copy, one write on blur, Enter, or page hide, and restore on reopen.
 */
export const attachmentDemoLabelEditor = createAttachmentLabelEditor({
  editing: attachmentDemoLabelEditing,
  draftKey: (batchKey) => ticketDraftKey('ux-demo', 'ticket-attachments-demo', 'attachment_label', batchKey),
  currentLabel: (edit) => attachmentDemoItems.value.find((item) => edit.ids?.includes(item.id))?.batch_label ?? '',
  describe: (batch) => ({ batch_id: batch.dataset.attachmentBatch || undefined }),
  save: (edit, label) => {
    renameAttachmentDemoBatch(edit.metadata?.batch_id ?? '', label);
    return Promise.resolve(true);
  },
});
const appendOnlyAttachmentDemoItems: TicketAttachmentItem[] = [
  { id: 'gh-trace', name: 'trace.log', url: 'data:text/plain,trace', batch_id: 'gh', batch_label: 'Repro' },
  // An append-only provider projects no annotations (HS2-HSA64D).
  { ...demoImages[0], id: 'gh-shot', annotationCount: 0, batch_id: 'gh', batch_label: 'Repro' },
];
export function TicketAttachmentsDemo() {
  const menu = attachmentDemoMenu.value;
  return (
    <>
      <CatalogExample
        label="Editable"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketAttachments
          attachments={attachmentDemoItems.value}
          editingLabelBatch={attachmentLabelEditingKey(
            attachmentDemoLabelEditing.value,
            INSPECTOR_ATTACHMENT_LABEL_SCOPE,
          )}
          editingLabelDraft={attachmentLabelEditingDraft(
            attachmentDemoLabelEditing.value,
            INSPECTOR_ATTACHMENT_LABEL_SCOPE,
          )}
        />
        {menu && <AttachmentContextMenu x={menu.x} y={menu.y} />}
      </CatalogExample>
      <CatalogExample
        label="Append-only provider (GitHub assets repository)"
        note="Files can be added; existing ones open by link and cannot be regrouped, relabelled, renamed, or deleted."
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketAttachments attachments={appendOnlyAttachmentDemoItems} editable={false} />
      </CatalogExample>
      <CatalogExample
        label="Provider without attachments"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketAttachments attachments={appendOnlyAttachmentDemoItems} enabled={false} />
      </CatalogExample>
      <CatalogExample
        label="Linked reader"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketAttachments
          attachments={appendOnlyAttachmentDemoItems}
          enabled={false}
          message="Attachments are view-only in linked ticket readers."
        />
      </CatalogExample>
    </>
  );
}
export function AttachmentGalleryDemo() {
  const video = galleryDemoUrl.value === demoImages[2].url,
    animatedSvg = galleryDemoUrl.value === demoImages[1].url,
    timed = video || animatedSvg,
    showcase = typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('annotation-shapes'),
    annotations = showcase
      ? shapeShowcaseAnnotations.map((annotation) =>
          timed ? { ...annotation, start_ms: 1000, end_ms: 3000 } : annotation,
        )
      : timed
        ? galleryDemoVideoAnnotations.value
        : imageAnnotations;
  return galleryDemoUrl.value ? (
    <AttachmentGallery
      images={demoImages}
      activeUrl={galleryDemoUrl.value}
      geometry={galleryDemoGeometry()}
      selectedScale={galleryDemoScale.value}
      annotations={annotations}
      markup={galleryDemoMarkup.value}
      drawMode={galleryDemoDrawMode.value}
      selectedAnnotation={showcase ? 'insert' : galleryDemoSelectedAnnotation.value}
      playheadMs={timed ? galleryDemoPlayhead.value : 0}
      durationMs={video ? 6000 : animatedSvg ? 4000 : 0}
      playing={galleryDemoPlaying.value}
      volume={galleryDemoVolume.value}
      muted={galleryDemoMuted.value}
      volumeOpen={galleryDemoVolumeOpen.value}
    />
  ) : (
    <button type="button" data-action="open-gallery-demo">
      Open gallery
    </button>
  );
}
export function TicketCodeReviewDemo() {
  return (
    <>
      <CatalogExample
        label="Configured — disjoint bundles"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketCodeReview
          review={{
            difftool: 'Glassbox',
            truncated: false,
            summary: {
              files: { total: 9, docs: 2, tests: 3, source: 3, other: 1 },
              tests_added: 2,
              tests_modified: 1,
            },
            ranges: [
              { from: 'aaa1111', to: 'bbb2222', count: 2 },
              { from: 'ccc3333', to: 'ddd4444', count: 2 },
            ],
            commits: [
              {
                sha: 'ddd4444',
                short_sha: 'ddd4444',
                subject: 'HS2-DEMO: complete the later review bundle',
                committed_at: '2026-09-02T10:00:00Z',
              },
              {
                sha: 'ccc3333',
                short_sha: 'ccc3333',
                subject: 'HS2-DEMO: begin the later review bundle',
                committed_at: '2026-09-02T09:00:00Z',
              },
              {
                sha: 'bbb2222',
                short_sha: 'bbb2222',
                subject: 'HS2-DEMO: complete the initial review bundle',
                committed_at: '2026-09-02T08:00:00Z',
              },
              {
                sha: 'aaa1111',
                short_sha: 'aaa1111',
                subject: 'HS2-DEMO: begin the initial review bundle',
                committed_at: '2026-09-02T07:00:00Z',
              },
            ],
          }}
        />
      </CatalogExample>
      <CatalogExample
        label="Remote device"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketCodeReview
          hostActionsAvailable={false}
          review={{
            difftool: 'Meld',
            truncated: false,
            ranges: [],
            commits: [
              {
                sha: 'bbb',
                short_sha: 'bbbbbbb',
                subject: 'HS2-DEMO: diff tool opens only on the host',
                committed_at: '2026-09-02T07:00:00Z',
              },
            ],
          }}
        />
      </CatalogExample>
      <CatalogExample
        label="Not configured"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketCodeReview
          review={{
            truncated: false,
            ranges: [],
            commits: [
              {
                sha: 'aaa',
                short_sha: 'aaaaaaa',
                subject: 'HS2-DEMO: readable without a tool',
                committed_at: '2026-09-02T07:00:00Z',
              },
            ],
          }}
        />
      </CatalogExample>
      <CatalogExample
        label="Empty"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketCodeReview review={{ difftool: 'Meld', truncated: false, ranges: [], commits: [] }} />
      </CatalogExample>
      <CatalogExample
        label="Loading / error"
        viewport={{ width: 'compact', frame: 'solid', surface: 'default', overflow: 'hidden', shadow: true }}
      >
        <TicketCodeReview loading message="The repository is temporarily unavailable." />
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireTicketMetadataDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'wa-hide', '[data-context-menu]', (event, target) => {
      if (event.target !== target || (target as HTMLElement).dataset.contextMenu !== 'attachment') return;
      window.setTimeout(closeAttachmentDemoMenu, 0);
    }),
  );
  demoListeners.add(
    listen(
      window,
      'pointerdown',
      (event) => {
        if (
          attachmentDemoMenu.value &&
          !(event.target as Element).closest(
            '[data-component="attachment-context-menu"], [data-action="open-attachment-menu"]',
          )
        )
          closeAttachmentDemoMenu();
      },
      { capture: true },
    ),
  );
  demoListeners.add(
    listen(window, 'keydown', (event) => {
      if (event.key === 'Escape') closeAttachmentDemoMenu();
    }),
  );
  // The production attachment gestures with fixture effects (HS2-1KM6MB).
  demoListeners.add(
    wireTicketAttachmentActions(root, {
      addFiles: (files, target) => {
        const added = Array.from(files).map((file, index) => ({
          id: `added-${Date.now()}-${index}`,
          name: file.name,
        }));
        if (selectedId.value === 'ticket-reader' || target.closest('[data-component="ticket-attachments"]'))
          readerAttachments.value = [...readerAttachments.value, ...added];
        recordCollectionEvent(
          `${added.length} attachment${added.length === 1 ? '' : 's'} added to ${target.closest<HTMLElement>('[data-ticket-slug]')?.dataset.ticketSlug ?? 'ticket'}`,
        );
      },
      labelEditor: attachmentDemoLabelEditor,
      regroup: (id, _source, destination) => {
        regroupAttachmentDemo(id, destination?.dataset.attachmentBatch);
      },
    }),
  );
  // The production gallery viewer controls with fixture effects (HS2-1KM6MB).
  demoListeners.add(
    wireAttachmentGalleryControls(root, {
      close: () => {
        setGalleryDemo(false);
      },
      shift: shiftGalleryDemo,
      zoom: zoomGalleryDemo,
      toggleMarkup: () => {
        galleryDemoMarkup.value = !galleryDemoMarkup.value;
        galleryDemoDrawMode.value = false;
      },
      toggleDraw: () => {
        galleryDemoDrawMode.value = !galleryDemoDrawMode.value;
      },
      togglePlayback: () => {
        galleryDemoPlaying.value = !galleryDemoPlaying.value;
      },
      playback: () => ({ playheadMs: galleryDemoPlayhead.value, durationMs: 6000 }),
      seek: (milliseconds, source) => {
        if (source === 'keyboard') galleryDemoPlaying.value = false;
        galleryDemoPlayhead.value = milliseconds;
      },
      seekAnnotation: (annotationId, milliseconds) => {
        galleryDemoPlayhead.value = Number.isFinite(milliseconds) ? milliseconds : 0;
        if (galleryDemoMarkup.value && annotationId) galleryDemoSelectedAnnotation.value = annotationId;
      },
      setVolume: (volume, commit) => {
        if (commit) return;
        galleryDemoVolume.value = volume;
        galleryDemoMuted.value = false;
      },
      toggleVolume: () => {
        galleryDemoVolumeOpen.value = !galleryDemoVolumeOpen.value;
      },
      toggleMuted: () => {
        galleryDemoMuted.value = !galleryDemoMuted.value;
      },
      beginRange: () => true,
      dragRange: (_annotationId, endpoint, fraction) => {
        setGalleryDemoAnnotationEndpoint(endpoint, fraction * 6000);
      },
      finishRange: () => undefined,
      nudgeRange: (_annotationId, endpoint, deltaMs) => {
        const annotation = galleryDemoVideoAnnotations.value[0];
        setGalleryDemoAnnotationEndpoint(
          endpoint,
          (endpoint === 'start' ? annotation.start_ms : annotation.end_ms) + deltaMs,
        );
      },
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openGalleryDemo.selector, () => {
      setGalleryDemo(true);
    }),
  );
  demoListeners.add(
    listen(root, 'click', (event) => {
      if (galleryDemoVolumeOpen.value && !(event.target as Element).closest('.attachment-gallery__volume'))
        galleryDemoVolumeOpen.value = false;
    }),
  );
  demoListeners.add(
    delegateCapture(root, 'pointerdown', DEMO_MARKERS.galleryAnnotationSurface.selector, (event) => {
      if (!galleryDemoMarkup.value || (event.target as Element).closest('[data-annotation-id]')) return;
      galleryDemoSelectedAnnotation.value = undefined;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openAttachmentMenu.selector, (event, target) => {
      event.stopPropagation();
      const rect = target.getBoundingClientRect();
      showAttachmentDemoMenu(rect.right, rect.bottom);
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'contextmenu',
      '[data-component="ticket-attachment-item"][data-attachment-menu-kind="item"]',
      (event) => {
        event.preventDefault();
        const pointer = event as MouseEvent;
        showAttachmentDemoMenu(pointer.clientX, pointer.clientY);
      },
    ),
  );
  demoListeners.add(
    listen(window, 'pagehide', () => {
      void attachmentDemoLabelEditor.flush();
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.attachmentMenuAction.selector, (_event, target) => {
      recordCollectionEvent(`${target.textContent.trim() || 'Attachment action'} selected`);
      closeAttachmentDemoMenu();
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-category-select': { catalogExamples: true, Demo: TicketCategorySelectDemo },
    'ticket-priority-select': { catalogExamples: true, Demo: TicketPrioritySelectDemo },
    'ticket-status-menu': { catalogExamples: true, Demo: TicketStatusMenuDemo },
    'ticket-info-panel': { catalogExamples: true, Demo: TicketInfoPanelDemo },
    'ticket-timeline': { catalogExamples: true, Demo: TicketTimelineDemo },
    'ticket-code-review': { catalogExamples: true, Demo: TicketCodeReviewDemo },
    'ticket-attachments': { catalogExamples: true, Demo: TicketAttachmentsDemo },
    'attachment-gallery': { Demo: AttachmentGalleryDemo },
  },
  wire: wireTicketMetadataDemo,
};
