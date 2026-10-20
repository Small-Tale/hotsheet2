/**
 * Optional observer for API requests (HS2-ZGBQ5Q). Production never installs one, so each request
 * pays only an `undefined` check; the development latency monitor installs one to time requests.
 */
export interface RequestMilestones {
  /** Elapsed from dispatch to response headers. */
  headers_ms?: number;
  /** Elapsed while reading and parsing the response body. */
  body_ms?: number;
}

export type RequestObserver = (
  method: string,
  path: string,
) => ((response: Response | undefined, milestones: RequestMilestones) => void) | undefined;

let observer: RequestObserver | undefined;

/** Install the observer; the returned function removes it again. */
export function setRequestObserver(next: RequestObserver): () => void {
  observer = next;
  return () => {
    if (observer === next) observer = undefined;
  };
}

/** Report a starting request; call the returned function once with its response (or `undefined`). */
export function observeRequest(
  method: string,
  path: string,
): ((response: Response | undefined, milestones: RequestMilestones) => void) | undefined {
  return observer?.(method, path);
}
