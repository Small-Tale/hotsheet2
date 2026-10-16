/**
 * Optional observer for API requests (HS2-ZGBQ5Q). Production never installs one, so each request
 * pays only an `undefined` check; the development latency monitor installs one to time requests.
 */
export type RequestObserver = (method: string, path: string) => ((response: Response | undefined) => void) | undefined;

let observer: RequestObserver | undefined;

/** Install the observer; the returned function removes it again. */
export function setRequestObserver(next: RequestObserver): () => void {
  observer = next;
  return () => {
    if (observer === next) observer = undefined;
  };
}

/** Report a starting request; call the returned function once with its response (or `undefined`). */
export function observeRequest(method: string, path: string): ((response: Response | undefined) => void) | undefined {
  return observer?.(method, path);
}
