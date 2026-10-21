import type { Attachment, MediaAnnotation } from './api';
import { restoreGalleryAnnotations } from './gallery-crop';
import type { GallerySource } from './interactions/types';

export interface GalleryAnnotationSession {
  projectId: string;
  ticketId: string;
  qualifiedId: string;
  attachmentId: string;
  before: MediaAnnotation[];
  original: MediaAnnotation[];
  crop?: Attachment['crop'];
  revision?: string;
  originalSize: { width: number; height: number };
}

export interface GalleryReset {
  url?: string;
  source?: GallerySource;
}

export interface GalleryAnnotationState {
  session?: GalleryAnnotationSession;
  pendingReset?: GalleryReset;
  saveState: 'idle' | 'saving' | 'failed';
  failure?: string;
}

export interface GallerySavePlan {
  session: GalleryAnnotationSession;
  annotations: MediaAnnotation[];
  crop?: Attachment['crop'];
  method: 'markup' | 'annotations';
  cropChanged: boolean;
}

export function beginGallerySession(
  state: GalleryAnnotationState,
  session: GalleryAnnotationSession,
): GalleryAnnotationState {
  return { ...state, session, saveState: 'idle', failure: undefined };
}

export function updateGallerySessionOriginal(
  state: GalleryAnnotationState,
  original: MediaAnnotation[],
  originalSize: { width: number; height: number },
): GalleryAnnotationState {
  return state.session ? { ...state, session: { ...state.session, original, originalSize } } : state;
}

export function queueGalleryReset(state: GalleryAnnotationState, reset: GalleryReset): GalleryAnnotationState {
  return state.session ? { ...state, pendingReset: reset } : state;
}

export type GalleryFinish =
  | { kind: 'none'; state: GalleryAnnotationState }
  | { kind: 'unchanged'; state: GalleryAnnotationState; reset?: GalleryReset }
  | { kind: 'save'; state: GalleryAnnotationState; plan: GallerySavePlan };

export function finishGallerySession(
  state: GalleryAnnotationState,
  annotations: readonly MediaAnnotation[],
  crop: Attachment['crop'],
  cropMode: boolean,
  currentUrl?: string,
): GalleryFinish {
  const session = state.session;
  if (!session || state.saveState === 'saving') return { kind: 'none', state };
  const copy = annotations.map((item) => structuredClone(item));
  const cropChanged = JSON.stringify(session.crop) !== JSON.stringify(crop);
  if (JSON.stringify(session.before) === JSON.stringify(copy) && !cropChanged) {
    return {
      kind: 'unchanged',
      state: { session: undefined, pendingReset: undefined, saveState: 'idle' },
      reset: state.pendingReset ?? (cropMode && currentUrl ? { url: currentUrl } : undefined),
    };
  }
  const canonical = cropMode
    ? copy
    : restoreGalleryAnnotations(session.original, copy, crop, session.originalSize.width, session.originalSize.height);
  return {
    kind: 'save',
    state: { ...state, saveState: 'saving', failure: undefined },
    plan: {
      session,
      annotations: canonical,
      crop,
      cropChanged,
      method: cropChanged || session.revision ? 'markup' : 'annotations',
    },
  };
}

export function failGallerySave(state: GalleryAnnotationState, message: string): GalleryAnnotationState {
  return { ...state, saveState: 'failed', failure: message };
}

export function completeGallerySave(state: GalleryAnnotationState): {
  state: GalleryAnnotationState;
  reset?: GalleryReset;
} {
  return {
    state: { session: undefined, pendingReset: undefined, saveState: 'idle' },
    reset: state.pendingReset,
  };
}

export function discardGallerySession(
  state: GalleryAnnotationState,
  currentUrl?: string,
): {
  state: GalleryAnnotationState;
  reset?: GalleryReset;
} {
  if (!state.session || state.saveState === 'saving') return { state };
  return {
    state: { session: undefined, pendingReset: undefined, saveState: 'idle' },
    reset: state.pendingReset ?? { url: currentUrl },
  };
}
