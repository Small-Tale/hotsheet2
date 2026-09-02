export interface AnimationFrameHost {
  requestAnimationFrame(callback: FrameRequestCallback): number;
  cancelAnimationFrame(handle: number): void;
}

export interface FrameBatcher {
  schedule(): void;
  flush(): void;
  cancel(): void;
}

/** Coalesces an arbitrary burst of input events into one visual update per frame. */
export function createFrameBatcher(host: AnimationFrameHost, update: () => void): FrameBatcher {
  let frame: number | undefined;
  const run = () => {
    frame = undefined;
    update();
  };
  return {
    schedule() {
      if (frame === undefined) frame = host.requestAnimationFrame(run);
    },
    flush() {
      if (frame !== undefined) host.cancelAnimationFrame(frame);
      frame = undefined;
      update();
    },
    cancel() {
      if (frame !== undefined) host.cancelAnimationFrame(frame);
      frame = undefined;
    },
  };
}
