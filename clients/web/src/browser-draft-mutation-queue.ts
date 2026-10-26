/** HS2-2HF3H4: capture command order before awaiting file bytes or opening IndexedDB. */
export function createBrowserDraftMutationQueue() {
  const tails = new Map<string, Promise<void>>();
  return (key: string, mutate: () => Promise<void>): Promise<void> => {
    const result = (tails.get(key) ?? Promise.resolve()).then(mutate),
      tail = result.then(
        () => undefined,
        () => undefined,
      );
    tails.set(key, tail);
    void tail.then(() => {
      if (tails.get(key) === tail) tails.delete(key);
    });
    return result;
  };
}
