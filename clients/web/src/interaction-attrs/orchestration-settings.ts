import { action } from 'kerfjs/actions';

/** Project Settings → General → Project Type and Settings → Orchestration (HS2-W27N1D). */
export const ORCHESTRATION_SETTINGS_ACTIONS = {
  addOpenProject: action('add-orchestration-project'),
  openAnotherProject: action('open-another-orchestration-project'),
  openProjectDetail: action('open-orchestration-project'),
  showProjectTab: action('show-orchestration-project-tab'),
  removeProject: action('remove-orchestration-project'),
  openSessionActions: action('open-orchestration-session-actions'),
  showSession: action('show-orchestration-session'),
  takeOverSession: action('take-over-orchestration-session'),
  stopSession: action('stop-orchestration-session'),
  applyPolicy: action('apply-orchestration-policy'),
  addPolicyAccount: action('add-orchestration-policy-account'),
  removePolicyAccount: action('remove-orchestration-policy-account'),
  applyProjectLimits: action('apply-orchestration-project-limits'),
  closeProjectDetail: action('close-orchestration-project'),
} as const;
