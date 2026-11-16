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

export declare const BRIDGE_OWNER_FILE: string;

export declare function recordBridgeOwner(input: {
  home: string;
  directories: string[];
  pid?: number;
  write?: (path: string, text: string) => void;
}): void;

export declare function processAlive(pid: number, kill?: (pid: number, signal: 0) => unknown): boolean;

export declare function sweepAbandonedBridgeHomes(input: {
  root: string;
  list?: (path: string) => string[];
  read?: (path: string) => string;
  alive?: (pid: number) => boolean;
  release?: (input: { home: string; directories: string[] }) => unknown;
}): string[];
