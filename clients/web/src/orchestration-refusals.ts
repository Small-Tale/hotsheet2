/**
 * HS2-8Z9VHH: every refusal or failure code the orchestration, session and remote surfaces return,
 * in words, with what to change. A refused action always says why on its first attempt and never
 * shows a bare code; the code stays as secondary detail at the end. `eslint-policy.test.ts` fails
 * when a code in the server's error enums has no entry here.
 */
import { type AdmissionRefusalDetail, ApiHttpError } from './api';

/** One refusal in words: what happened, and what to do about it when the person can do something. */
export interface RefusalText {
  readonly message: string;
  readonly next?: string;
}

const ORCHESTRATION_SETTINGS = 'Settings › Orchestration';
const MACHINE_DEFAULTS = `${ORCHESTRATION_SETTINGS} › Machine Defaults`;
const RETRY = 'Try again in a moment.';
const READ_AGAIN = 'Wait for the view to refresh, then try again.';

/** The audit: each code, its message and next step. Keys are the server's stable codes. */
export const ORCHESTRATION_REFUSALS: Readonly<Record<string, RefusalText>> = {
  // OrchestrationError
  invalid_request: { message: 'The request was not valid.', next: 'Check the values and try again.' },
  conflict: { message: 'The orchestration changed meanwhile.', next: READ_AGAIN },
  target_not_found: {
    message: 'That project is not part of this orchestration project.',
    next: `Add it in ${ORCHESTRATION_SETTINGS}.`,
  },
  target_removed: {
    message: 'That project was removed from this orchestration project.',
    next: `Add it again in ${ORCHESTRATION_SETTINGS}.`,
  },
  upgrade_required: {
    message: 'This orchestration data was written by a newer Hot Sheet.',
    next: 'Update Hot Sheet on this machine.',
  },
  corrupt_orchestration_data: {
    message: 'The orchestration data on disk is damaged.',
    next: 'Restore it from Git, or remove the damaged file.',
  },
  capacity_reached: { message: 'The orchestration reached its record limit.', next: 'Remove finished work first.' },
  storage_unavailable: { message: 'The orchestration files could not be read or written.', next: RETRY },
  forbidden: { message: 'This operation is never allowed for the one who asked.' },
  // Admission reasons (the machine's own limits)
  tool_not_installed: {
    message: 'The AI tool is not installed on that machine.',
    next: 'Install and sign in to it there, or choose another tool.',
  },
  tool_not_configured: {
    message: 'That project opted out of the AI tool.',
    next: 'Choose another tool, or allow it in the project.',
  },
  // HS2-ZRPZJJ: the machine reports the tool signed out; an unknown sign-in is still tried.
  tool_signed_out: {
    message: 'The AI tool is signed out on that machine.',
    next: 'Sign in to it there, or choose another tool.',
  },
  session_cap_reached: {
    message: 'The machine already runs as many AI sessions as its limit allows.',
    next: `Close a session, or raise Sessions per Machine in ${MACHINE_DEFAULTS}.`,
  },
  above_cpu_ceiling: {
    message: 'The machine’s CPU load is above its ceiling.',
    next: `Wait for the load to drop, or raise CPU Ceiling in ${MACHINE_DEFAULTS}.`,
  },
  below_disk_floor: {
    message: 'The machine has less free disk than its floor.',
    next: `Free disk space, or lower Free Disk Floor in ${MACHINE_DEFAULTS}.`,
  },
  below_ram_floor: {
    message: 'The machine has too little free memory for another AI session.',
    next: `Close apps or sessions, or lower Free Memory Floor in ${MACHINE_DEFAULTS}.`,
  },
  capacity_unknown: {
    message: 'The machine could not measure its capacity, and unknown measurements are not allowed.',
    next: RETRY,
  },
  // DenialReason
  target_unavailable: { message: 'That project cannot be reached now.', next: 'Open it, or check its machine.' },
  policy_denied: {
    message: 'The orchestration policy does not allow it.',
    next: `Review the policy in ${ORCHESTRATION_SETTINGS}.`,
  },
  permission_required: {
    message: 'It waits for a person to answer a permission request.',
    next: 'Answer it in Notifications.',
  },
  permission_denied: { message: 'A person denied it.' },
  decision_required: {
    message: 'It waits for a decision on the orchestration ticket.',
    next: 'Answer the feedback request there.',
  },
  // PolicyDenial (the policy's own limits)
  policy_absent: {
    message: 'This orchestration project has no policy yet.',
    next: `Save one in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_target_not_permitted: {
    message: 'The policy does not include that project.',
    next: `Add it in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_target_disabled: {
    message: 'That project is turned off in the policy.',
    next: `Turn it on in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_tool_not_permitted: {
    message: 'The policy does not allow that AI tool.',
    next: `Give the tool sessions under Tools in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_model_not_permitted: {
    message: 'The policy does not allow that model.',
    next: `Allow it under Tools in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_load_unknown: { message: 'The current session load could not be counted.', next: RETRY },
  policy_target_cap: {
    message: 'That project already runs as many sessions as the policy allows.',
    next: `Raise its limit in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_machine_cap: {
    message: 'The machine already runs as many sessions as the policy allows.',
    next: `Raise Sessions per Machine in ${MACHINE_DEFAULTS}.`,
  },
  policy_tool_cap: {
    message: 'The AI tool already runs as many sessions as the policy allows.',
    next: `Raise its Sessions under Tools in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_account_not_permitted: {
    message: 'The policy does not allow that account.',
    next: `Add the account under Tools in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_account_cap: {
    message: 'The account already runs as many sessions as the policy allows.',
    next: `Raise its sessions under Tools in ${ORCHESTRATION_SETTINGS}.`,
  },
  policy_speculation_not_allowed: { message: 'The policy does not allow building ahead on unconfirmed work.' },
  policy_project_software: {
    message: 'This project is a Software project.',
    next: 'Change its Project Type to Orchestration in Project Settings › General.',
  },
  // TargetFailure (reading a project)
  unreachable: { message: 'The project’s machine did not answer.', next: 'Check that it is on and connected.' },
  unauthorized: { message: 'The project’s machine refused this server.', next: 'Pair the machines again.' },
  source_missing: { message: 'The project’s ticket source is missing.', next: 'Open the project and link its source.' },
  identity_changed: {
    message: 'The project now answers as a different checkout.',
    next: `Remove it and add it again in ${ORCHESTRATION_SETTINGS}.`,
  },
  unsupported_metadata: {
    message: 'The project was written by a different Hot Sheet version.',
    next: 'Update Hot Sheet on both machines.',
  },
  missing_credentials: { message: 'This server has no credentials for that project.', next: 'Pair the machines.' },
  revoked: { message: 'Access to that project was revoked.', next: 'Pair the machines again.' },
  trust_failed: { message: 'The project’s machine could not be verified.', next: 'Pair the machines again.' },
  redirect_refused: { message: 'The project’s machine redirected the request, which is not allowed.' },
  response_limit: { message: 'The project’s answer was too large.' },
  invalid_response: { message: 'The project’s machine answered with something unreadable.', next: RETRY },
  disabled_source: { message: 'The project’s ticket source is turned off.', next: 'Turn it on in Ticket Sources.' },
  review_required: { message: 'The project needs review before it can be used.', next: 'Open it and review it.' },
  // SiteError
  site_project_unknown: { message: 'That project is no longer added.' },
  site_chain: { message: 'A site cannot have sites of its own.' },
  site_project_has_sites: {
    message: 'That project has sites of its own.',
    next: 'Separate its sites first.',
  },
  site_repository_mismatch: { message: 'It declares a different repository than this project.' },
  site_store_mismatch: { message: 'It reaches a different ticket store than this project.' },
  site_plan_conflict: { message: 'The plan would have two steps on one ticket of the project.' },
  site_run_conflict: { message: 'Two active runs would work on one ticket of the project.' },
  // GateError
  gate_not_found: { message: 'That gate does not exist.' },
  gate_provider_unsupported: { message: 'The project’s provider does not support that gate.' },
  gate_provider_unavailable: { message: 'The gate’s provider could not be reached.', next: RETRY },
  gate_not_passed: { message: 'The gate has not passed yet.' },
  gate_invalid_transition: { message: 'The gate cannot change that way from its current state.', next: READ_AGAIN },
  gate_repository_mismatch: { message: 'The gate names a different repository.' },
  // HS2-YA5S82: the host's Job monitor has not read the pipeline's repository yet; the gate stays waiting.
  gate_repository_unknown: {
    message: 'The host has not read which repository this pipeline builds yet.',
    next: RETRY,
  },
  gate_deployment_active: { message: 'A deployment is already running.', next: 'Wait for it to finish.' },
  gate_artifact_mismatch: { message: 'The artifact does not match the one the gate checked.' },
  gate_deployment_script_untracked: {
    message: 'The deployment script is not committed.',
    next: 'Commit it, then try again.',
  },
  // TargetOpError (a project's own ticket source or peer)
  target_ticket_not_found: { message: 'The ticket was not found in that project.' },
  target_operation_unsupported: { message: 'That project’s ticket source does not support this.' },
  target_conflict: { message: 'The ticket changed in that project meanwhile.', next: READ_AGAIN },
  target_provider_unauthorized: {
    message: 'That project’s ticket source refused the credentials.',
    next: 'Sign in again in Ticket Sources.',
  },
  target_rate_limited: { message: 'That project’s ticket source is rate limiting requests.', next: RETRY },
  target_source_disabled: { message: 'That project’s ticket source is turned off.' },
  target_request_invalid: { message: 'That project’s ticket source rejected the request.' },
  target_peer_unsupported: {
    message: 'The project’s machine runs a Hot Sheet that does not support this.',
    next: 'Update Hot Sheet there.',
  },
  orchestration_links_unsupported: { message: 'That project’s ticket source cannot store orchestration links.' },
  target_operation_failed: { message: 'That project’s ticket source could not do it.', next: RETRY },
  target_peer_session_expired: { message: 'The session with the project’s machine expired.', next: 'Sign in again.' },
  target_peer_session_revoked: {
    message: 'The project’s machine revoked this connection.',
    next: 'Pair the machines again.',
  },
  target_peer_read_only: { message: 'The project’s machine allows reading only.' },
  target_peer_csrf_required: {
    message: 'The project’s machine needs a fresh session.',
    next: 'Reload, then try again.',
  },
  target_peer_auth_refused: {
    message: 'The project’s machine refused to sign in.',
    next: 'Pair the machines again.',
  },
  // ReadinessError and planning
  readiness_step_not_found: { message: 'That plan step no longer exists.', next: READ_AGAIN },
  readiness_blocked: { message: 'That plan step waits for its prerequisites.' },
  readiness_stale: { message: 'That plan step changed meanwhile.', next: READ_AGAIN },
  readiness_cyclic: { message: 'The plan’s prerequisites form a cycle.', next: 'Update the plan.' },
  readiness_provisional: { message: 'That plan step builds on unconfirmed work.' },
  readiness_override_denied: { message: 'Starting that step early is not allowed.' },
  // HS2-061YTB: a manual dispatch to a machine that does not meet, or did not report, a step requirement.
  requirement_unmet: {
    message: "The target's machine does not meet a requirement of this step.",
    next: 'Dispatch it to a machine that meets it, or change the step requirements.',
  },
  requirement_unknown: {
    message: "The target's machine has not reported a requirement of this step.",
    next: 'Wait for the machine to report its capabilities, then try again.',
  },
  readiness_provisional_refused: { message: 'The policy does not allow building on unconfirmed work.' },
  readiness_not_provisional: { message: 'That plan step does not build on unconfirmed work.' },
  plan_no_targets: {
    message: 'This project has no projects to plan for.',
    next: `Add some in ${ORCHESTRATION_SETTINGS}.`,
  },
  plan_no_goals: {
    message: 'This project has no Git ticket source for its goal tickets.',
    next: 'Add one in Ticket Sources.',
  },
  plan_goal_not_open: { message: 'That goal ticket is not open.' },
  plan_conflict: { message: 'The plan changed meanwhile.', next: READ_AGAIN },
  plan_update_not_found: { message: 'No plan update is in progress.' },
  plan_update_not_open: { message: 'No plan update is in progress.' },
  plan_update_conflict: { message: 'The plan update changed meanwhile.', next: READ_AGAIN },
  plan_update_invalid: { message: 'The plan update is not valid.', next: 'Ask the planner to update the plan again.' },
  plan_update_planner_busy: {
    message: 'The planning session is still working.',
    next: 'Try again when it finishes its turn.',
  },
  // TemplateError (HS2-A4RVYA)
  template_not_found: { message: 'That template no longer exists.', next: READ_AGAIN },
  template_retired: { message: 'That template was retired.', next: 'Choose another template.' },
  template_invalid: {
    message: 'The template definition is not valid.',
    next: 'Correct the template, then save it again.',
  },
  template_conflict: { message: 'The templates changed meanwhile.', next: READ_AGAIN },
  template_inputs_invalid: { message: 'An input of the template is missing or not valid.', next: 'Check the inputs.' },
  template_target_unknown: {
    message: 'A step names a project this orchestration project does not include.',
    next: `Choose another project, or add it in ${ORCHESTRATION_SETTINGS}.`,
  },
  template_apply_conflict: {
    message: 'This apply was already made with other inputs.',
    next: 'Apply the template again from the start.',
  },
  template_apply_not_found: { message: 'That apply is not known on this machine.' },
  template_goal_empty: {
    message: 'That goal has no plan steps to save.',
    next: 'Propose a plan for it first.',
  },
  // HS2-36XDDX: Accept Plan before the planner defined any step for the template's goal.
  template_accept_nothing_planned: {
    message: 'The planner has not planned any step for this template’s goal yet.',
    next: 'Wait until its proposed plan shows, then accept it.',
  },
  // TriggerError (HS2-KXKA7X)
  trigger_not_found: { message: 'That trigger no longer exists on this machine.', next: READ_AGAIN },
  trigger_invalid: {
    message: 'The trigger is not complete or not valid.',
    next: 'Check what it watches and what it runs, then save it again.',
  },
  trigger_not_firing: {
    message: 'The trigger is paused or waits for a review, so it does not run.',
    next: 'Resume it, or review it and turn it on.',
  },
  trigger_template_missing: {
    message: 'The template this trigger runs no longer exists or was retired.',
    next: 'Edit the trigger and choose another template.',
  },
  // RunError
  run_target_active: { message: 'A run already works on that ticket.' },
  run_worker_busy: { message: 'That worker is busy with another run.' },
  run_not_active: { message: 'That run is not active.', next: READ_AGAIN },
  run_invalid_transition: { message: 'The run cannot change that way from its current state.', next: READ_AGAIN },
  run_not_found: { message: 'That run no longer exists.', next: READ_AGAIN },
  run_paused: { message: 'That run is paused.', next: 'Resume it first.' },
  // SessionError
  session_not_found: { message: 'That session no longer exists.', next: READ_AGAIN },
  session_not_active: { message: 'That session is not running.', next: READ_AGAIN },
  session_invalid_transition: {
    message: 'The session cannot change that way from its current state.',
    next: READ_AGAIN,
  },
  session_human_owned: { message: 'A person took that session over, so the orchestrator leaves it alone.' },
  session_not_safe: {
    message: 'The session is in the middle of work.',
    next: 'Wait for a safe point, or stop it anyway.',
  },
  session_force_not_permitted: { message: 'Only a person may stop a session in the middle of its work.' },
  session_host_unavailable: {
    message: 'The machine’s terminal host did not answer.',
    next: 'Check that Hot Sheet runs there, then try again.',
  },
  session_admission_refused: {
    message: 'The machine refused to start another AI session.',
    next: `Review its limits in ${MACHINE_DEFAULTS}.`,
  },
  session_role_active: { message: 'A session in that role is already running.' },
  session_start_failed: { message: 'The AI tool could not be started.', next: 'Open its terminal to see why.' },
  tool_setup_failed: {
    message: 'The AI tool could not be set up in the project.',
    next: 'Run its setup in the project, then try again.',
  },
  session_run_held: { message: 'The session still holds a run.', next: 'Finish or hand off the run first.' },
  session_withdrawn: { message: 'The session start was withdrawn.' },
  session_mid_turn: { message: 'The session is in the middle of a turn.', next: 'Try again when it finishes.' },
  // ValidationError
  validation_step_not_found: { message: 'That validation step no longer exists.', next: READ_AGAIN },
  validation_step_running: { message: 'That validation step is already running.' },
  validation_result_not_found: { message: 'That validation result no longer exists.', next: READ_AGAIN },
  validation_result_final: { message: 'That validation result is already final.' },
  validation_invalid_transition: {
    message: 'The validation cannot change that way from its current state.',
    next: READ_AGAIN,
  },
};

const sessions = (count: number) => `${String(count)} AI ${count === 1 ? 'session' : 'sessions'}`;

/** The machine's refusal with its numbers, and where to change the limit. */
export function admissionRefusalText(detail: AdmissionRefusalDetail): RefusalText {
  // A newer host may send a reason this client does not know yet.
  const known = refusalText(detail.reason);
  const { limit, observed } = detail;
  const both = limit !== undefined && observed !== undefined;
  switch (detail.reason) {
    case 'session_cap_reached':
      if (limit === 0)
        return { message: 'The machine’s session limit is 0, so no AI session may start.', next: known?.next };
      if (both)
        return {
          message: `The machine already runs ${sessions(observed)}; its limit is ${String(limit)}.`,
          next: known?.next,
        };
      break;
    case 'above_cpu_ceiling':
      if (both)
        return { message: `CPU load is ${String(observed)}%, above the ${String(limit)}% ceiling.`, next: known?.next };
      break;
    case 'below_disk_floor':
      if (both)
        return {
          message: `${String(observed)} MB of disk is free, below the ${String(limit)} MB floor.`,
          next: known?.next,
        };
      break;
    case 'below_ram_floor':
      if (both)
        return {
          message: `${String(observed)} MB of memory is free, which leaves no room for a session above the ${String(limit)} MB floor.`,
          next: known?.next,
        };
      break;
    case 'tool_not_installed':
    case 'tool_not_configured':
    case 'tool_signed_out':
      if (detail.tool)
        return {
          message:
            detail.reason === 'tool_not_installed'
              ? `${detail.tool} is not installed on that machine.`
              : detail.reason === 'tool_signed_out'
                ? `${detail.tool} is signed out on that machine.`
                : `That project opted out of ${detail.tool}.`,
          next: known?.next,
        };
      break;
  }
  return known ?? ORCHESTRATION_REFUSALS.session_admission_refused;
}

/** A refusal's words for a code, or undefined for a code this client does not know. */
export function refusalText(code: string | undefined): RefusalText | undefined {
  return code && Object.hasOwn(ORCHESTRATION_REFUSALS, code) ? ORCHESTRATION_REFUSALS[code] : undefined;
}

const joined = (text: RefusalText) => (text.next ? `${text.message} ${text.next}` : text.message);

/**
 * The words for any failure of an orchestration, session or remote action: the mapped message and next
 * step, with the code as secondary detail; an unknown code keeps the server's own message. `action`
 * names what failed ("Propose Plan"), so the toast reads on its own.
 */
export function describeRefusal(reason: unknown, action?: string): string {
  const prefix = action ? `${action}: ` : '';
  if (reason instanceof ApiHttpError) {
    const text = reason.admission ? admissionRefusalText(reason.admission) : refusalText(reason.code);
    if (text) return `${prefix}${joined(text)} (${reason.admission?.reason ?? reason.code})`;
    return `${prefix}${reason.message}`;
  }
  return `${prefix}${reason instanceof Error ? reason.message : String(reason)}`;
}
