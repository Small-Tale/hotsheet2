export declare const WEBKIT_REPEAT_OVERRIDE: string;

type SpecEngines = Record<string, { launched: string[]; optIn: string[] }>;

export declare function playwrightTestArgs(argv: string[]): string[] | null;

export declare function localWebkitUse(input: {
  args: string[];
  env: Record<string, string | undefined>;
  specs: SpecEngines;
}): { repeatEach: number; list: boolean; webkit: string[] };

export declare function webkitRepeatGuard(input: {
  argv: string[];
  env: Record<string, string | undefined>;
  specs: SpecEngines;
}): { refuse: string | null; warn: string | null };

export declare function runnerWebkitGuard(
  argv?: string[],
  env?: Record<string, string | undefined>,
): { refuse: string | null; warn: string | null };
