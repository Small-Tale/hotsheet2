export declare function localPlaywrightWorkers(input: {
  cpus: number;
  load1: number;
  override?: string;
  ci?: boolean;
}): number | undefined;

export declare const DEFAULT_PLAYWRIGHT_NICE: number;

export declare function playwrightNiceness(input: {
  current: number;
  override?: string;
  ci?: boolean;
}): number | undefined;

export declare function lowerPlaywrightPriority(
  os: { getPriority: () => number; setPriority: (priority: number) => void },
  env: Record<string, string | undefined>,
): void;
