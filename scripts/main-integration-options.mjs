// HS2-BND42N: keep the main writer's target and gate selection explicit.
const fullSha = /^[0-9a-f]{40}$/i;
const allowedGitFlags = new Set(['--quiet', '-q', '--verbose', '-v', '--porcelain', '--atomic']);
const valuedGateFlags = new Set(['--ticket', '--t2-stage', '--base', '--budget', '--tier']);

// HS2-GT57BQ: the ci-staged integration branch; the Rust policy validates the same shape.
export function validStagingBranch(name) {
  return (
    typeof name === 'string' &&
    name.length <= 100 &&
    /^[A-Za-z0-9][A-Za-z0-9._/-]*$/.test(name) &&
    !name.includes('..') &&
    !name.includes('//') &&
    !/[/.]$/.test(name) &&
    !name.endsWith('.lock') &&
    name !== 'main' &&
    name !== 'HEAD'
  );
}

export function parseMainPushArgs(args) {
  const result = {
    remote: 'origin',
    source: 'HEAD',
    target: 'refs/heads/main',
    gitFlags: [],
    gateArgs: [],
    sourceStore: null,
    identity: null,
    assignment: null,
    // HS2-9G0M3V: a batch push names every integrating member and the tickets it moves.
    assignments: [],
    phaseTickets: [],
    standalone: false,
    // HS2-GT57BQ: `main` (default), `staged` (push HEAD to the integration branch) or
    // `promote` (fast-forward main to an exact green staged SHA).
    mode: 'main',
    stagingBranch: null,
    promote: null,
  };
  const identity = {};
  const positionals = [];
  let explicitSource = false;
  const next = (index, flag) => {
    const value = args[index + 1];
    if (!value || value.startsWith('--')) throw new Error(`${flag} requires a value`);
    return value;
  };
  for (let index = 0; index < args.length; index++) {
    const value = args[index];
    if (allowedGitFlags.has(value)) result.gitFlags.push(value);
    else if (value === '--source') {
      result.source = next(index++, value);
      explicitSource = true;
    } else if (value === '--source-store') result.sourceStore = next(index++, value);
    else if (value === '--integrator-id') identity.id = next(index++, value);
    else if (value === '--integrator-session') identity.session = next(index++, value);
    else if (value === '--assignment') result.assignments.push(next(index++, value));
    else if (value === '--phase-ticket') result.phaseTickets.push(next(index++, value));
    else if (value === '--standalone') result.standalone = true;
    else if (value === '--staged') result.mode = result.mode === 'main' ? 'staged' : 'conflict';
    else if (value === '--staging-branch') result.stagingBranch = next(index++, value);
    else if (value === '--promote') {
      result.promote = next(index++, value);
      result.mode = result.mode === 'main' ? 'promote' : 'conflict';
    }
    else if (value === '--gate-arg' || value.startsWith('--gate-arg=')) {
      const gateArg = value === '--gate-arg' ? args[++index] : value.slice(11);
      if (!gateArg) throw new Error('--gate-arg requires a value');
      result.gateArgs.push(gateArg);
    } else if (value.startsWith('-')) throw new Error(`Unsupported push option: ${value}`);
    else positionals.push(value);
  }
  if (result.mode === 'conflict') throw new Error('--staged and --promote cannot be combined');
  if (result.stagingBranch !== null && result.mode !== 'staged')
    throw new Error('--staging-branch requires --staged');
  if (positionals.length > 2) throw new Error('Push exactly one main ref to one named remote');
  if (positionals[0]) result.remote = positionals[0];
  if (positionals[1]) {
    if (explicitSource) throw new Error('Use --source or a refspec, not both');
    if (positionals[1] === 'main') result.source = 'main';
    else {
      const [source, target, extra] = positionals[1].split(':');
      if (extra !== undefined || !source || !target) throw new Error('Invalid main refspec');
      result.source = source;
      result.target = target;
    }
  }
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(result.remote)) throw new Error('Use a named Git remote, not a URL or path');
  if (!['HEAD', 'main', 'refs/heads/main'].includes(result.source) && !fullSha.test(result.source))
    throw new Error('Push the checked-out HEAD only');
  if (!['main', 'refs/heads/main'].includes(result.target)) throw new Error('Only main is supported');
  result.target = 'refs/heads/main';
  if (result.mode === 'staged') {
    if (positionals[1]) throw new Error('A staged push sends the checked-out HEAD; omit the refspec');
    result.stagingBranch ??= 'integration';
    if (!validStagingBranch(result.stagingBranch)) throw new Error('Invalid staging branch');
    result.target = `refs/heads/${result.stagingBranch}`;
  }
  if (result.mode === 'promote') {
    if (!fullSha.test(result.promote)) throw new Error('--promote requires the full green staged SHA');
    if (explicitSource || positionals[1]) throw new Error('--promote names its SHA; omit --source and the refspec');
    if (result.gateArgs.length) throw new Error('--promote runs no local gate; its green CI run is the gate');
    if (result.assignments.length || result.phaseTickets.length)
      throw new Error('--promote moves no assignment; record it with coordination batch-promote');
    result.promote = result.promote.toLowerCase();
    result.source = result.promote;
  }
  if (Boolean(identity.id) !== Boolean(identity.session))
    throw new Error('Integrator id and session must be supplied together');
  if (new Set(result.assignments).size !== result.assignments.length) throw new Error('Duplicate --assignment');
  result.assignment = result.assignments[0] ?? null;
  if (result.assignment && !identity.id) throw new Error('Assignment requires integrator identity');
  if (result.phaseTickets.length && !result.assignment) throw new Error('--phase-ticket requires --assignment');
  if (result.standalone && identity.id) throw new Error('Standalone conflicts with integrator identity');
  if (result.mode !== 'main' && !identity.id) throw new Error(`--${result.mode} requires integrator identity`);
  if (identity.id) result.identity = identity;
  const singleGateFlags = new Set(['--full', '--smoke', '--ticket', '--base', '--budget', '--tier']);
  const seenGateFlags = new Set();
  for (let index = 0; index < result.gateArgs.length; index++) {
    const arg = result.gateArgs[index];
    if (singleGateFlags.has(arg)) {
      if (seenGateFlags.has(arg)) throw new Error(`Duplicate gate option: ${arg}`);
      seenGateFlags.add(arg);
    }
    if (arg === '--full' || arg === '--smoke') continue;
    if (!valuedGateFlags.has(arg)) throw new Error(`Unsupported gate option: ${arg}`);
    if (!result.gateArgs[++index] || result.gateArgs[index].startsWith('--'))
      throw new Error(`${arg} requires a value`);
  }
  // HS2-9G0M3V: the smoke profile is one declared check set, never combined with selection flags.
  if (
    seenGateFlags.has('--smoke') &&
    result.gateArgs.some((arg) => ['--full', '--ticket', '--tier', '--t2-stage', '--budget'].includes(arg))
  )
    throw new Error('--smoke cannot be combined with --full, --ticket, --tier, --t2-stage or --budget');
  if (result.assignments.length > 1 && !seenGateFlags.has('--smoke'))
    throw new Error('A batch push of several assignments runs the --smoke gate');
  return result;
}
