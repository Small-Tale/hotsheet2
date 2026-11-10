import type { CustomDevelopmentStrategy, DevelopmentStrategyPolicy, DevelopmentStrategyReport } from '../api';

export const NEW_CUSTOM = '__new_custom__';

export interface DevelopmentStrategyDraft {
  selection: string;
  name: string;
  greenGoal: string;
  sessionOverride: string;
  ticketOverride: string;
  custom: {
    integration: CustomDevelopmentStrategy['integration'];
    local_gate: CustomDevelopmentStrategy['local_gate'];
    minimum_tier: CustomDevelopmentStrategy['minimum_tier'];
    budget_minutes: string;
    browser: boolean;
    platform: boolean;
    perf: boolean;
    test_plan_timing: CustomDevelopmentStrategy['test_plan_timing'];
    ci_gate: string;
    staging_branch: string;
    green_main_goal_minutes: string;
    red_ci_recovery: CustomDevelopmentStrategy['red_ci_recovery'];
  };
}

const DEFAULT_CUSTOM: CustomDevelopmentStrategy = {
  integration: 'serial',
  local_gate: 'targeted',
  minimum_tier: 'T1',
  browser: false,
  platform: false,
  perf: false,
  test_plan_timing: 'before-handoff',
  ci_gate: 'CI',
  red_ci_recovery: 'attribute-and-retry',
};

function customDraft(profile: CustomDevelopmentStrategy = DEFAULT_CUSTOM): DevelopmentStrategyDraft['custom'] {
  return {
    ...profile,
    budget_minutes: profile.budget_minutes?.toString() ?? '',
    staging_branch: profile.staging_branch ?? '',
    green_main_goal_minutes: profile.green_main_goal_minutes?.toString() ?? '',
  };
}

export function draftFromDevelopmentReport(report: DevelopmentStrategyReport): DevelopmentStrategyDraft {
  const selection = report.policy.default_custom ?? report.policy.default;
  return {
    selection,
    name: report.policy.default_custom ?? '',
    greenGoal: String(report.policy.green_main_goal_minutes),
    sessionOverride: '',
    ticketOverride: '',
    custom: customDraft(report.policy.custom[selection]),
  };
}

export function selectDevelopmentStrategy(
  draft: DevelopmentStrategyDraft,
  selection: string,
  policy: DevelopmentStrategyPolicy,
): DevelopmentStrategyDraft {
  return {
    ...draft,
    selection,
    name: selection === NEW_CUSTOM ? '' : policy.custom[selection] ? selection : draft.name,
    custom: customDraft(policy.custom[selection]),
  };
}

export function changeDevelopmentStrategyField(
  draft: DevelopmentStrategyDraft,
  field: string,
  value: string | boolean,
): DevelopmentStrategyDraft {
  if (field === 'selection') return { ...draft, selection: String(value) };
  if (field === 'name') return { ...draft, name: String(value) };
  if (field === 'greenGoal') return { ...draft, greenGoal: String(value) };
  if (field === 'sessionOverride' || field === 'ticketOverride') return { ...draft, [field]: String(value) };
  if (!(field in draft.custom)) return draft;
  if (field === 'integration' && value !== 'ci-staged')
    return {
      ...draft,
      custom: { ...draft.custom, integration: value as CustomDevelopmentStrategy['integration'], staging_branch: '' },
    };
  return { ...draft, custom: { ...draft.custom, [field]: value } };
}

function optionalMinutes(value: string, label: string, max: number): number | undefined {
  if (!value.trim()) return undefined;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 1 || parsed > max) throw new Error(`${label} must be 1–${max} minutes.`);
  return parsed;
}

export function policyFromDevelopmentDraft(
  existing: DevelopmentStrategyPolicy,
  draft: DevelopmentStrategyDraft,
): DevelopmentStrategyPolicy {
  const greenMain = optionalMinutes(draft.greenGoal, 'Green main goal', 10_080);
  if (greenMain === undefined) throw new Error('Green main goal is required.');
  const policy: DevelopmentStrategyPolicy = {
    ...existing,
    version: 2,
    green_main_goal_minutes: greenMain,
    custom: { ...existing.custom },
  };
  if (draft.selection === 'ticket-assured' || draft.selection === 'steady-main' || draft.selection === 'risk-guided') {
    policy.default = draft.selection;
    delete policy.default_custom;
    return policy;
  }
  const name = (draft.selection === NEW_CUSTOM ? draft.name : draft.selection).trim();
  if (!/^[a-z](?:[a-z0-9]|-(?!-))*$/.test(name) || name.endsWith('-') || name.length > 40)
    throw new Error('Custom strategy name must be 1–40 characters of lowercase kebab-case.');
  if (['ticket-assured', 'steady-main', 'risk-guided'].includes(name))
    throw new Error('Choose a distinct custom strategy name.');
  const profile: CustomDevelopmentStrategy = {
    integration: draft.custom.integration,
    local_gate: draft.custom.local_gate,
    minimum_tier: draft.custom.minimum_tier,
    browser: draft.custom.browser,
    platform: draft.custom.platform,
    perf: draft.custom.perf,
    test_plan_timing: draft.custom.test_plan_timing,
    ci_gate: draft.custom.ci_gate,
    red_ci_recovery: draft.custom.red_ci_recovery,
  };
  const budget = optionalMinutes(draft.custom.budget_minutes, 'Local budget', 1_440);
  if (budget !== undefined) profile.budget_minutes = budget;
  const customGoal = optionalMinutes(draft.custom.green_main_goal_minutes, 'Custom green main goal', 10_080);
  if (customGoal !== undefined) profile.green_main_goal_minutes = customGoal;
  if (draft.custom.staging_branch.trim()) profile.staging_branch = draft.custom.staging_branch.trim();
  policy.custom[name] = profile;
  policy.default_custom = name;
  return policy;
}
