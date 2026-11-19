export declare const BROWSER_SLOT_NAMESPACE: string;
export declare const DEFAULT_WEBKIT_SLOTS: number;
export declare const SLOTS_HELD_ENV: string;
export declare const DEFAULT_LEASE_MS: number;
export declare const DEFAULT_WAIT_MS: number;
export declare const WAIT_ROUND_MS: number;

type Env = Record<string, string | undefined>;

export interface BrowserSlot {
  resource: string | null;
  claimId: string | null;
  reason?: string;
  waitedMs?: number;
  release: () => Promise<void>;
  releaseSync: () => void;
}

export declare function slotPolicy(engine: string, env: Env): 'on' | 'off' | 'held';
export declare function slotNames(engine: string, env: Env): string[];
export declare function claimCli(env: Env): string;
export declare function parseClaimOutput(stdout: string): Record<string, unknown> | null;
export declare function describeHolders(holders: Array<Record<string, unknown>>): string;

export declare function acquireBrowserSlot(options: {
  engine: string;
  env?: Env;
  waitMs?: number;
  leaseMs?: number;
  purpose?: string;
  spawn?: unknown;
  warn?: (message: string) => void;
  now?: () => number;
}): Promise<BrowserSlot>;

export declare function releaseBrowserSlotSync(token: string, env?: Env, spawnSync?: unknown): void;
export declare function browserSlotPoolAvailable(env?: Env, spawnSync?: unknown): boolean;

export declare function holdRunnerWebkitSlots(input: {
  args: string[] | null;
  env: Env;
  workers?: number;
  acquire?: typeof acquireBrowserSlot;
  onExit?: (release: () => void) => void;
}): Promise<{ workers: number; slots: BrowserSlot[] } | null>;
