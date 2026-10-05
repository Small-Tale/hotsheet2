import type { ToastState } from './toast-lifetime';

export interface PresentedToast {
  readonly isConnected: boolean;
  hide(): Promise<void>;
  remove(): void;
}

/** Coordinate the public asynchronous toast factory with application replacement and disposal. */
export function createToastPresentation<Item extends PresentedToast>(adapter: {
  create: (message: string) => Promise<Item>;
  clear: () => void;
  prepare: (item: Item, generation: number) => void;
}) {
  let desired: ToastState = { message: '', generation: 0 },
    active: Item | undefined,
    disposed = false;
  const isDisposed = () => disposed;
  return {
    async publish(state: ToastState): Promise<void> {
      if (isDisposed()) return;
      desired = state;
      if (!state.message) {
        const closing = active;
        active = undefined;
        await closing?.hide();
        return;
      }
      active = undefined;
      adapter.clear();
      const item = await adapter.create(state.message);
      if (isDisposed() || desired.generation !== state.generation || !desired.message || !item.isConnected) {
        item.remove();
        return;
      }
      adapter.prepare(item, state.generation);
      active = item;
    },
    dismissed(item: unknown): number | undefined {
      if (item !== active) return;
      active = undefined;
      desired = { ...desired, message: '' };
      return desired.generation;
    },
    dispose(): void {
      disposed = true;
      active = undefined;
      adapter.clear();
    },
  };
}
