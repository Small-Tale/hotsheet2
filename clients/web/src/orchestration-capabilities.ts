/**
 * HS2-ZY4Z4G: the words a target machine's capabilities read as in its project detail. Pure: the
 * capabilities come from the capacity snapshot (docs/26 §26.10), and a member the machine did not
 * report reads as unknown, never as absent.
 */
import type { OrchestrationCapabilities } from './api';

const OS_NAMES: Record<OrchestrationCapabilities['os'], string> = {
  macos: 'macOS',
  linux: 'Linux',
  windows: 'Windows',
  other: 'Other OS',
};

const GPU_NAMES: Record<string, string> = {
  amd: 'AMD',
  apple: 'Apple',
  intel: 'Intel',
  nvidia: 'NVIDIA',
  qualcomm: 'Qualcomm',
  other: 'Other',
};

/** `macOS 15.1, arm64` or `Linux (ubuntu 24.04), x86_64`. */
export function systemLabel(capabilities: OrchestrationCapabilities): string {
  const name = OS_NAMES[capabilities.os];
  const release = capabilities.os_distribution
    ? ` (${[capabilities.os_distribution, capabilities.os_version].filter(Boolean).join(' ')})`
    : capabilities.os_version
      ? ` ${capabilities.os_version}`
      : '';
  return `${name}${release}, ${capabilities.arch}`;
}

/** The GPU vendors, `None` without one, `Unknown` when the machine could not tell. */
export function gpuLabel(capabilities: OrchestrationCapabilities): string {
  const gpu = capabilities.gpu;
  if (!gpu) return 'Unknown';
  if (!gpu.present) return 'None';
  return (gpu.vendors ?? []).map((vendor) => GPU_NAMES[vendor] ?? vendor).join(', ') || 'Present';
}

/** The installed toolchains with versions, `None installed`, or `Unknown`. */
export function toolchainsLabel(capabilities: OrchestrationCapabilities): string {
  const toolchains = capabilities.toolchains;
  if (!toolchains) return 'Unknown';
  const installed = toolchains.filter((toolchain) => toolchain.installed);
  if (installed.length === 0) return 'None installed';
  return installed.map((toolchain) => [toolchain.name, toolchain.version].filter(Boolean).join(' ')).join(', ');
}

/** The installed AI tools with versions, by their display names, or `Unknown`. */
export function aiToolsLabel(
  capabilities: OrchestrationCapabilities,
  toolLabel: (tool: string) => string = (tool) => tool,
): string {
  const tools = capabilities.ai_tools;
  if (!tools) return 'Unknown';
  if (tools.length === 0) return 'None installed';
  return tools.map((tool) => [toolLabel(tool.tool), tool.version].filter(Boolean).join(' ')).join(', ');
}
