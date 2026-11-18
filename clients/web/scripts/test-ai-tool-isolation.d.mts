export declare const AI_TOOL_PROBES_ENV: 'HOTSHEET_AI_TOOL_PROBES';
export declare function isolateAiToolProbes<T extends Record<string, string | undefined>>(env?: T): T;
export declare const HOST_HEALTH_SAMPLER_ENV: 'HOTSHEET_HOST_HEALTH_SAMPLER';
export declare function isolateHostHealthSampler<T extends Record<string, string | undefined>>(env?: T): T;
export declare const MACHINE_HOME_ENV: Readonly<
  Record<
    | 'HOTSHEET_HOME'
    | 'HOTSHEET_LEGACY_HOME'
    | 'HOTSHEET_CACHE_DIR'
    | 'HOTSHEET_CLAUDE_USER_CONFIG'
    | 'HOTSHEET_LAUNCH_AGENTS_DIR',
    string
  >
>;
export declare function isolateMachineHome<T extends Record<string, string | undefined>>(root: string, env?: T): T;
export declare const NATIVE_SECRETS_ENV: 'HOTSHEET_NATIVE_SECRETS';
export declare function isolateNativeSecrets<T extends Record<string, string | undefined>>(env?: T): T;
export declare const LAUNCHED_SESSION_ENV: readonly [
  'HOTSHEET_SERVER',
  'HOTSHEET_SECRET',
  'HOTSHEET_TERMINAL_ID',
  'HOTSHEET_PROJECT',
  'HOTSHEET_PERMISSION_PROJECT',
  'HOTSHEET_WORKER_ID',
  'HOTSHEET_ACTOR_ROLE',
  'HOTSHEET_ACTOR_ID',
  'HOTSHEET_AGENT',
];
export declare function isolateLaunchedSession<T extends Record<string, string | undefined>>(env?: T): T;
