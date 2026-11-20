//! HS2-XYF4YT: provisional prerequisites in the answer of an uncoordinated claim.
//!
//! Inform only: `claim`, `claim-next` and `renew` claim exactly what they claimed before.
//! When the project's committed `speculation` policy is on and the claimed ticket has a
//! prerequisite awaiting CI, the answer names it, its `CI gate commit` (the base to build
//! on) and the policy's integration rule. A prerequisite whose recorded gate reads red is
//! reported once per claim: a machine-local record keeps a repeat `renew` quiet. With the
//! policy off, or nothing to report, the answer is unchanged. The rule is
//! `hotsheet_ticketing::provisional::claim_prerequisites`; the CLI, the server routes and
//! the MCP serverless backend all call [`claim_provisional`].

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use hotsheet_model::{ClaimEventKind, Ticket};
use hotsheet_ticketing::checkouts::{self, Checkout, CheckoutRegistry};
use hotsheet_ticketing::provisional::{
    ClaimPrerequisite, GateObservation, GateProbe, IntegrationRule, ProvisionalBase,
    SpeculationPolicy, claim_prerequisites,
};
use hotsheet_ticketing::{FileLock, FsStore, Settings};
use serde::{Deserialize, Serialize};

use crate::coordination::speculation_runtime::LiveGateProbe;

/// The machine-local record of red gates already reported to a claim.
pub const WARNINGS_FILE: &str = "provisional-claim-warnings.json";
/// The newest records kept; an older claim that warns again after eviction is harmless.
pub const MAX_WARNINGS: usize = 1024;
const VERSION: u32 = 1;

/// The code checkout a claim's prerequisites are judged in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvisionalContext {
    /// The shipping checkout, which holds the committed policy and the Git history.
    pub project: PathBuf,
    /// Its checkout id, which keys its CI watch Jobs.
    pub checkout_id: String,
    /// The machine home: the Job store and the warning record live under it.
    pub home: PathBuf,
}

impl ProvisionalContext {
    /// The code checkout containing `dir`, resolved to its shipping checkout. `None`
    /// when `dir` is not inside a Git checkout.
    pub fn for_dir(dir: &Path, home: &Path) -> Option<Self> {
        let output = hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let common = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
        let shipping = hotsheet_sync::paths::canonicalize(common.parent()?).ok()?;
        Some(Self {
            checkout_id: checkouts::checkout_id(&shipping).ok()?,
            project: shipping,
            home: home.to_path_buf(),
        })
    }

    /// A registered checkout.
    pub fn for_checkout(checkout: &Checkout, home: &Path) -> Self {
        Self {
            project: PathBuf::from(&checkout.root),
            checkout_id: checkout.id.clone(),
            home: home.to_path_buf(),
        }
    }

    /// The one registered checkout that links the ticket store at `store_root`. `None`
    /// when no checkout or more than one links it, since the code history is ambiguous.
    pub fn for_store(store_root: &Path, home: &Path) -> Option<Self> {
        let store = hotsheet_sync::paths::canonicalize(store_root).ok()?;
        let linked: Vec<Checkout> = CheckoutRegistry::new(home.join("checkouts.json"))
            .list()
            .ok()?
            .into_iter()
            .filter(|checkout| {
                checkout
                    .sources
                    .iter()
                    .filter(|source| source.provider == "git")
                    .map(|source| PathBuf::from(&source.locator))
                    .chain(checkout.stores.iter().map(PathBuf::from))
                    .any(|path| {
                        hotsheet_sync::paths::canonicalize(&path).is_ok_and(|path| path == store)
                    })
            })
            .collect();
        match linked.as_slice() {
            [only] => Some(Self::for_checkout(only, home)),
            _ => None,
        }
    }
}

/// A prerequisite whose recorded gate reads red.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedGate {
    /// The prerequisite's slug.
    pub slug: String,
    pub gate_commit: String,
    pub gate: GateObservation,
}

/// The `provisional` member of a claim or renew answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimProvisional {
    /// Where the dependent commit may go while a base gate is pending.
    pub integration: IntegrationRule,
    /// Prerequisites awaiting CI: build on a base that contains each `base_commit`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<ProvisionalBase>,
    /// Red gates not yet reported to this claim: rebase onto the repair, or reopen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub red_gates: Vec<RedGate>,
}

impl ClaimProvisional {
    /// The lines the CLI prints after its usual answer.
    pub fn lines(&self) -> Vec<String> {
        let rule = match self.integration {
            IntegrationRule::PushPerTicket => {
                "push-per-ticket: it may land once its own gates pass, and the covering CI run gates it with its base"
            }
            IntegrationRule::Parked => "parked: keep it on its branch until the base gate is green",
        };
        let mut lines: Vec<String> = self
            .prerequisites
            .iter()
            .map(|base| {
                format!(
                    "Prerequisite {} awaits CI ({}): build on a base that contains its CI gate commit {}; integrate {rule}.",
                    base.slug,
                    gate_text(&base.gate),
                    base.base_commit
                )
            })
            .collect();
        lines.extend(self.red_gates.iter().map(|red| {
            format!(
                "Warning: the CI gate of prerequisite {} reads red ({}) at {}: rebase onto its repair, or reopen this work.",
                red.slug,
                gate_text(&red.gate),
                red.gate_commit
            )
        }));
        lines
    }
}

fn gate_text(gate: &GateObservation) -> String {
    match &gate.run {
        Some(run) => format!("{} run {run}", gate.result.name()),
        None => gate.result.name().to_owned(),
    }
}

/// The current claim attempt: the newest `claim` event in the ticket's history.
fn claim_attempt(ticket: &Ticket) -> Option<String> {
    ticket
        .claim_history
        .iter()
        .rfind(|event| event.kind == ClaimEventKind::Claim)
        .map(|event| event.id.to_string())
}

#[derive(Default, Serialize, Deserialize)]
struct WarningRecord {
    version: u32,
    /// `<ticket>/<claim attempt>/<prerequisite>/<gate commit>/<run>`, oldest first.
    #[serde(default)]
    warned: Vec<String>,
}

/// Record `keys` under `home` and return those not recorded before, in order. Best
/// effort: when the record cannot be read or written, every key is returned, so a
/// failure repeats a warning rather than losing one.
pub fn warn_once(home: &Path, keys: &[String]) -> Vec<String> {
    if keys.is_empty() {
        return Vec::new();
    }
    let attempt = || -> anyhow::Result<Vec<String>> {
        fs::create_dir_all(home)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(home.join(format!("{WARNINGS_FILE}.lock")))?;
        let _guard = FileLock::acquire(lock)?;
        let path = home.join(WARNINGS_FILE);
        let mut record: WarningRecord = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => WarningRecord::default(),
            Err(error) => return Err(error.into()),
        };
        let fresh: Vec<String> = keys
            .iter()
            .filter(|key| !record.warned.contains(key))
            .cloned()
            .collect();
        if fresh.is_empty() {
            return Ok(fresh);
        }
        record.version = VERSION;
        record.warned.extend(fresh.iter().cloned());
        let excess = record.warned.len().saturating_sub(MAX_WARNINGS);
        record.warned.drain(..excess);
        let staged = home.join(format!("{WARNINGS_FILE}.tmp"));
        fs::write(&staged, serde_json::to_vec_pretty(&record)?)?;
        fs::rename(&staged, &path)?;
        Ok(fresh)
    };
    attempt().unwrap_or_else(|_| keys.to_vec())
}

/// Shape the rule's answer for one claim: awaiting-CI prerequisites always, red gates
/// only the first time this claim attempt sees each (prerequisite, commit, run).
pub fn shape(
    policy: &SpeculationPolicy,
    claimed: &Ticket,
    found: Vec<ClaimPrerequisite>,
    home: &Path,
) -> Option<ClaimProvisional> {
    let mut prerequisites = Vec::new();
    let mut red = Vec::new();
    for entry in found {
        match entry {
            ClaimPrerequisite::AwaitingCi(base) => prerequisites.push(base),
            ClaimPrerequisite::GateRed { .. } => red.push(entry),
        }
    }
    let attempt = claim_attempt(claimed).unwrap_or_else(|| "-".to_owned());
    let keyed: Vec<(String, ClaimPrerequisite)> = red
        .into_iter()
        .filter_map(|entry| {
            let key = format!("{}/{attempt}/{}", claimed.id, entry.red_key()?);
            Some((key, entry))
        })
        .collect();
    let fresh = warn_once(
        home,
        &keyed.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>(),
    );
    let red_gates: Vec<RedGate> = keyed
        .into_iter()
        .filter(|(key, _)| fresh.contains(key))
        .filter_map(|(_, entry)| match entry {
            ClaimPrerequisite::GateRed {
                slug,
                gate_commit,
                gate,
                ..
            } => Some(RedGate {
                slug,
                gate_commit,
                gate,
            }),
            ClaimPrerequisite::AwaitingCi(_) => None,
        })
        .collect();
    (!prerequisites.is_empty() || !red_gates.is_empty()).then_some(ClaimProvisional {
        integration: policy.integration,
        prerequisites,
        red_gates,
    })
}

/// The `provisional` member for `claimed`, judged with `probe`. `None` when the policy
/// is off or unreadable, the ticket has no prerequisite, or nothing is reported.
pub fn claim_provisional_with(
    context: &ProvisionalContext,
    store: &FsStore,
    claimed: &Ticket,
    probe: &dyn Fn(&SpeculationPolicy) -> Box<dyn GateProbe>,
) -> Option<ClaimProvisional> {
    if claimed.blocked_by.is_empty() {
        return None;
    }
    let policy = SpeculationPolicy::read(&Settings::for_project(&context.project)).ok()?;
    if !policy.enabled {
        return None;
    }
    let tickets = store.list_tickets().ok()?;
    let probe = probe(&policy);
    let found = claim_prerequisites(&policy, claimed, &tickets, probe.as_ref());
    shape(&policy, claimed, found, &context.home)
}

/// [`claim_provisional_with`] with the live probe: Git reachability in the checkout and
/// the machine's CI watch Jobs. It never contacts the CI provider.
pub fn claim_provisional(
    context: Option<&ProvisionalContext>,
    store: &FsStore,
    claimed: &Ticket,
) -> Option<ClaimProvisional> {
    let context = context?;
    claim_provisional_with(context, store, claimed, &|policy| {
        Box::new(LiveGateProbe::open(
            &context.project,
            &context.home,
            &context.checkout_id,
            policy,
        ))
    })
}

/// Add the `provisional` member to a JSON claim answer (a ticket object, or a resolved
/// `{ store, ticket }`), leaving it byte-identical when there is nothing to report.
pub fn attach(answer: &mut serde_json::Value, provisional: Option<&ClaimProvisional>) {
    if let (Some(provisional), Some(object)) = (provisional, answer.as_object_mut())
        && let Ok(value) = serde_json::to_value(provisional)
    {
        object.insert("provisional".to_owned(), value);
    }
}

#[cfg(test)]
#[path = "provisional_claims_tests.rs"]
mod provisional_claims_tests;
