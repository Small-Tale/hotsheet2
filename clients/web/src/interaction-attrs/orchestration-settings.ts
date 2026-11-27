import { action } from 'kerfjs/actions';

/** Project Settings → General → Project Type and Settings → Orchestration (HS2-W27N1D). */
export const ORCHESTRATION_SETTINGS_ACTIONS = {
  addOpenProject: action('add-orchestration-project'),
  openAnotherProject: action('open-another-orchestration-project'),
  /** HS2-92FJXM: one of the serving machine's open checkouts, for a project open from another server. */
  addHostedProject: action('add-hosted-orchestration-project'),
  openProjectDetail: action('open-orchestration-project'),
  /** HS2-M1T6KN: a project group's disclosure arrow in the Projects tree, and Right and Left on its row. */
  toggleProjectGroup: action('toggle-orchestration-project-group'),
  showProjectTab: action('show-orchestration-project-tab'),
  removeProject: action('remove-orchestration-project'),
  /** HS2-E9VVQF: a project row's Add a Site → an open project, and its Show All Projects choice. */
  addSite: action('add-orchestration-site'),
  showAllSites: action('show-all-orchestration-site-choices'),
  openAnotherSite: action('open-another-orchestration-site'),
  /** HS2-E9VVQF: a site row's Separate from <project>. */
  separateSite: action('separate-orchestration-site'),
  /** HS2-NAA3HV: a site row's Make <site> the Home Site. */
  makeHomeSite: action('make-orchestration-home-site'),
  openSessionActions: action('open-orchestration-session-actions'),
  showSession: action('show-orchestration-session'),
  takeOverSession: action('take-over-orchestration-session'),
  stopSession: action('stop-orchestration-session'),
  applyPolicy: action('apply-orchestration-policy'),
  addPolicyAccount: action('add-orchestration-policy-account'),
  removePolicyAccount: action('remove-orchestration-policy-account'),
  applyProjectLimits: action('apply-orchestration-project-limits'),
  /** HS2-Y737GM: the Limits header's help popup. */
  showLimitsHelp: action('show-orchestration-limits-help'),
  /** HS2-1X1TXC, HS2-NPAZQW: a chosen model or resource's remove button. */
  removeLimitsValue: action('remove-orchestration-limits-value'),
  closeProjectDetail: action('close-orchestration-project'),
} as const;
