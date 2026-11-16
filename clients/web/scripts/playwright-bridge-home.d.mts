import type { spawnSync } from 'node:child_process';

export declare function bridgeCliBinary(env: NodeJS.ProcessEnv, cwd?: string, platform?: NodeJS.Platform): string;

export declare function releaseBridgeHome(input: {
  home: string;
  directories: string[];
  env?: NodeJS.ProcessEnv;
  cli?: string;
  exists?: (path: string) => boolean;
  run?: typeof spawnSync;
  remove?: (path: string) => void;
  warn?: (message: string) => void;
}): { stopped: boolean; error?: string };
