import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';

import type { TerminalLimits } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

/** The Select names the save handler reads (HS2-Y5DYC0). */
export const TERMINAL_LIMIT_FIELDS = {
  historyLines: 'terminal-history-lines',
  replayBytes: 'terminal-replay-bytes',
  warmTerminals: 'terminal-warm-terminals',
} as const;
/** The choice value meaning "use the memory-derived default". */
export const TERMINAL_LIMIT_DEFAULT = 'default';

const HISTORY_CHOICES = [1_000, 2_500, 5_000, 10_000, 20_000, 50_000];
const REPLAY_CHOICES = [256, 512, 1_024, 2_048, 4_096, 8_192, 16_384].map((kib) => kib * 1024);
const WARM_CHOICES = [0, 2, 4, 8, 12, 16, 24, 32];

export interface TerminalMemorySettingsProps {
  /** The server's answer; absent while loading or after a failure. */
  limits?: TerminalLimits;
  /** The defaults this device applies instead of the server machine's, when it runs on another device or
   * a phone; absent when the server's memory-derived defaults apply here. */
  deviceDefaults?: { historyLines: number; warmTerminals: number };
  message?: string;
}

const lines = (value: number) => `${value.toLocaleString('en-US')} lines`;
const terminals = (value: number) => (value === 1 ? '1 terminal' : `${value.toLocaleString('en-US')} terminals`);
/** Bytes as KiB/MiB/GiB with at most one decimal. */
export function formatMemoryBytes(bytes: number): string {
  const units = ['bytes', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes,
    unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const rounded = unit === 0 || Number.isInteger(value) ? String(Math.round(value)) : value.toFixed(1);
  return `${rounded.replace(/\.0$/, '')} ${units[unit]}`;
}

/** Choices for one limit: the default (named with its value), the presets, and any other configured value. */
export function terminalLimitChoices(
  presets: readonly number[],
  defaultValue: number,
  configured: number | null | undefined,
  format: (value: number) => string,
): SelectChoice[] {
  const values =
    configured == null || presets.includes(configured) ? presets : [...presets, configured].sort((a, b) => a - b);
  return [
    { value: TERMINAL_LIMIT_DEFAULT, label: `Default (${format(defaultValue)})` },
    ...values.map((value, index) => ({ value: String(value), label: format(value), separatorBefore: index === 0 })),
  ];
}

/**
 * App Settings → Terminal memory (HS2-Y5DYC0): the machine-wide history, server replay buffer, and
 * warm-terminal limits. Each defaults from the physical memory the server reports; an explicit choice
 * always wins. Choices save immediately.
 */
export function TerminalMemorySettings({ limits, deviceDefaults, message = '' }: TerminalMemorySettingsProps) {
  const defaults = limits?.defaults,
    configured = limits?.configured,
    loading = !limits,
    selected = (value: number | null | undefined) => (value == null ? TERMINAL_LIMIT_DEFAULT : String(value)),
    memory = limits?.physical_memory_bytes;
  return (
    <div data-component="terminal-memory-settings" {...COMMANDS_AND_AI_ACTIONS.saveTerminalLimits.attrs}>
      <List gap="l">
        <List gap="xs">
          <Text variant="h2">Terminal memory</Text>
          <Text tone="quiet">
            {memory
              ? `Defaults are sized for this machine’s ${formatMemoryBytes(memory)} of memory. `
              : limits
                ? 'This machine did not report its memory, so the defaults are conservative. '
                : ''}
            These limits apply to every project on this machine and take effect for terminals opened afterward.
          </Text>
          {deviceDefaults ? (
            <Text tone="quiet" size="compact" data-role="terminal-limits-conservative">
              This device keeps smaller defaults because it is not the server’s machine. A choice you make here still
              applies on every device.
            </Text>
          ) : null}
        </List>
        <Grid minColumnWidth={rem(15)} gap="m">
          <Select
            name={TERMINAL_LIMIT_FIELDS.historyLines}
            label="Terminal history"
            hint="Lines a terminal keeps to scroll back through. A magnified dashboard card keeps a fifth."
            value={selected(configured?.history_lines)}
            disabled={loading}
            placeholder={loading}
            choices={terminalLimitChoices(
              HISTORY_CHOICES,
              deviceDefaults?.historyLines ?? defaults?.history_lines ?? 0,
              configured?.history_lines,
              lines,
            )}
          />
          <Select
            name={TERMINAL_LIMIT_FIELDS.replayBytes}
            label="Server replay buffer"
            hint="Recent output the server keeps per terminal and replays after a reconnect. The default follows the history."
            value={selected(configured?.replay_bytes)}
            disabled={loading}
            placeholder={loading}
            choices={terminalLimitChoices(
              REPLAY_CHOICES,
              defaults?.replay_bytes ?? 0,
              configured?.replay_bytes,
              formatMemoryBytes,
            )}
          />
          <Select
            name={TERMINAL_LIMIT_FIELDS.warmTerminals}
            label="Warm terminals"
            hint="Terminals kept running in the background after you switch away, so they reappear instantly."
            value={selected(configured?.warm_terminals)}
            disabled={loading}
            placeholder={loading}
            choices={terminalLimitChoices(
              WARM_CHOICES,
              deviceDefaults?.warmTerminals ?? defaults?.warm_terminals ?? 0,
              configured?.warm_terminals,
              terminals,
            )}
          />
        </Grid>
        <Text variant="span" tone="danger" size="compact" role="status">
          {message}
        </Text>
      </List>
    </div>
  );
}
