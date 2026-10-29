import type { Note } from './api';

export interface FeedbackResponseScope {
  projectId: string;
  qualifiedId: string;
  surface: string;
}
export interface FeedbackResponseOwner extends FeedbackResponseScope {
  request: Note;
  /** Stable only until this response surface is closed, reset, or replaced. */
  readonly lifecycle: symbol;
  /** Latest response input, including text not yet committed by the typing-pause coalescer. */
  draft?: string;
}
const key = (scope: FeedbackResponseScope) => JSON.stringify([scope.projectId, scope.qualifiedId, scope.surface]);

/** Local response purpose survives refreshed request kinds; no owner is server authority (HS2-VBFSZJ). */
export function createFeedbackResponseDrafts() {
  // Purpose metadata changes alongside existing coalesced editor signals. It must not
  // trigger a render before those signals commit the current keystroke (HS2-VBFSZJ).
  const owners = new Map<string, FeedbackResponseOwner>();
  return {
    get(scope: FeedbackResponseScope) {
      return owners.get(key(scope));
    },
    begin(scope: FeedbackResponseScope, request: Note, draft?: string) {
      const existing = owners.get(key(scope));
      if (existing?.request.id === request.id) return existing;
      const owner = { ...scope, request: { ...request }, draft, lifecycle: Symbol() };
      owners.set(key(scope), owner);
      return owner;
    },
    touch(owner: FeedbackResponseOwner, draft = owner.draft) {
      if (owners.get(key(owner)) !== owner) return owner;
      const next = { ...owner, draft };
      owners.set(key(owner), next);
      return next;
    },
    acceptsInput(owner: FeedbackResponseOwner) {
      const current = owners.get(key(owner));
      return current?.lifecycle === owner.lifecycle && current.draft === owner.draft;
    },
    isCurrent(owner: FeedbackResponseOwner) {
      return owners.get(key(owner)) === owner;
    },
    clear(owner: FeedbackResponseOwner | undefined) {
      if (!owner || owners.get(key(owner)) !== owner) return false;
      owners.delete(key(owner));
      return true;
    },
    retain(scopes: readonly FeedbackResponseScope[]) {
      const live = new Set(scopes.map(key));
      for (const scope of owners.keys()) if (!live.has(scope)) owners.delete(scope);
    },
  };
}
export type FeedbackResponseDrafts = ReturnType<typeof createFeedbackResponseDrafts>;
