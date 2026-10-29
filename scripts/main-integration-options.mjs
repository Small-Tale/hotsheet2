// HS2-BND42N: keep the main writer's target and gate selection explicit.
const fullSha = /^[0-9a-f]{40}$/i;
const allowedGitFlags = new Set(['--quiet', '-q', '--verbose', '-v', '--porcelain', '--atomic']);
const valuedGateFlags = new Set(['--ticket', '--t2-stage', '--base', '--budget', '--tier']);

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
    standalone: false,
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
    else if (value === '--assignment') result.assignment = next(index++, value);
    else if (value === '--standalone') result.standalone = true;
    else if (value === '--gate-arg' || value.startsWith('--gate-arg=')) {
      const gateArg = value === '--gate-arg' ? args[++index] : value.slice(11);
      if (!gateArg) throw new Error('--gate-arg requires a value');
      result.gateArgs.push(gateArg);
    } else if (value.startsWith('-')) throw new Error(`Unsupported push option: ${value}`);
    else positionals.push(value);
  }
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
  if (Boolean(identity.id) !== Boolean(identity.session))
    throw new Error('Integrator id and session must be supplied together');
  if (result.assignment && !identity.id) throw new Error('Assignment requires integrator identity');
  if (result.standalone && identity.id) throw new Error('Standalone conflicts with integrator identity');
  if (identity.id) result.identity = identity;
  const singleGateFlags = new Set(['--full', '--ticket', '--base', '--budget', '--tier']);
  const seenGateFlags = new Set();
  for (let index = 0; index < result.gateArgs.length; index++) {
    const arg = result.gateArgs[index];
    if (singleGateFlags.has(arg)) {
      if (seenGateFlags.has(arg)) throw new Error(`Duplicate gate option: ${arg}`);
      seenGateFlags.add(arg);
    }
    if (arg === '--full') continue;
    if (!valuedGateFlags.has(arg)) throw new Error(`Unsupported gate option: ${arg}`);
    if (!result.gateArgs[++index] || result.gateArgs[index].startsWith('--'))
      throw new Error(`${arg} requires a value`);
  }
  return result;
}
