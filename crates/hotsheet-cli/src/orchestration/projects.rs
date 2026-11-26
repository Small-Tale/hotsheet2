//! HS2-W27N1D: the project type and the projects an orchestration project follows.
//!
//! These are the workflows behind Project Settings → General → Project Type and
//! Settings → Orchestration → Projects. The web client, the CLI and MCP call them
//! through the one router, so every client switches the type and adds or removes a
//! project the same way.
//!
//! Adding a project declares a target (§26.2) and binds it to an open checkout on this
//! machine or on a reviewed remote server in one step. Adding is the consent to read
//! and follow that project (HS2-XHQ1EH): no grant is written here, and whatever the
//! orchestrator later does there passes the authorization gate as before.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::checkouts::Checkout;
use hotsheet_ticketing::orchestration::{
    AddTarget, AuditOutcome, AuditRecord, BindTarget, DeclarationState, InspectDestination,
    InspectTarget, PROJECT_TYPE_SETTING, ProjectType, ProjectTypeView, RemoveTarget,
    ReplaceTargets, SetProjectType, TargetDestination, TargetResolution, TypeState, UnbindTarget,
    plan_type_change,
};
use hotsheet_ticketing::{Scope, Settings, SettingsError};
use serde::{Deserialize, Serialize};

use super::*;

/// Add one open project to an orchestration project.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddProject {
    /// The open checkout: on this machine, or on a reviewed remote registration.
    pub destination: InspectDestination,
    /// The name the orchestration project shows; defaults to the checkout id.
    #[serde(default)]
    pub label: Option<String>,
    /// A canonical ULID the caller keeps across retries of this one add, so a retry
    /// after a failed bind binds the same target instead of declaring another.
    #[serde(default)]
    pub target_id: Option<String>,
    /// HS2-E9VVQF: add it as another site of this active project target (§26.2,
    /// HS2-SHH7WW). A checkout that reaches another ticket store than the project's
    /// bound sites is refused with `site_store_mismatch` before anything is declared.
    #[serde(default)]
    pub site_of: Option<String>,
}

/// HS2-92FJXM: add a checkout this server already has open, as a client on another
/// machine may ask for it. Only the checkout's id on this server is named: no remote
/// destination, and nothing new is opened.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddHostedProject {
    pub checkout_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    /// HS2-89YNGN: add it as another site of this active project target, with the
    /// same refusal before anything is declared as the local add (`site_store_mismatch`).
    #[serde(default)]
    pub site_of: Option<String>,
}

/// HS2-92FJXM: one checkout this server has open, as + Add Project lists it for a
/// client on another machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostedProjectChoice {
    pub checkout_id: String,
    pub name: String,
    /// Already added to the orchestration project and bound to this checkout.
    pub added: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    /// HS2-89YNGN: the ticket store the checkout reaches, as the non-secret digest a
    /// binding records (`checkout_authority`), so a client can offer the open projects
    /// of one project's store as its sites and notice a duplicate. Absent when this
    /// server could not read the checkout's sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout_authority: Option<String>,
}

/// HS2-92FJXM: why a checkout cannot be added from another machine.
pub const CHECKOUT_NOT_OPEN: &str = "only a checkout this server already has open can be added";

/// Remove one project. Nothing else is needed: the current revisions are read under
/// the same request, and a stale one still conflicts.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveProject {}

/// One added project as the workflows report it.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectView {
    pub target_id: String,
    pub label: String,
    /// False when the project was already added and bound to this checkout.
    pub added: bool,
    /// The fresh binding the add confirmed, as `target resolve` reports it.
    pub resolution: TargetResolution,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemovedProject {
    pub target_id: String,
    /// This machine's binding was removed by this request.
    pub unbound: bool,
    /// The declaration was tombstoned by this request (false when it already was).
    pub removed: bool,
}

fn settings_error(error: SettingsError) -> OrchestrationError {
    match error {
        SettingsError::Io(error) => OrchestrationError::Storage(error),
        SettingsError::UpgradeRequired { found, .. } => OrchestrationError::UnsupportedVersion {
            found: found.parse().unwrap_or(0),
        },
        SettingsError::Parse { .. } | SettingsError::Invalid { .. } => OrchestrationError::Corrupt,
    }
}

/// The same place: kind, remote registration and checkout. Identity details may have
/// been reviewed since, which a fresh bind confirms.
fn same_place(bound: &TargetDestination, wanted: &InspectDestination) -> bool {
    match (bound, wanted) {
        (
            TargetDestination::Local { checkout_id, .. },
            InspectDestination::Local {
                checkout_id: wanted,
            },
        ) => checkout_id == wanted,
        (
            TargetDestination::Remote {
                registration_id,
                checkout_id,
                ..
            },
            InspectDestination::Remote {
                registration_id: wanted_registration,
                checkout_id: wanted_checkout,
            },
        ) => registration_id == wanted_registration && checkout_id == wanted_checkout,
        _ => false,
    }
}

impl OrchestrationRuntime {
    fn project_settings(owner: &Checkout) -> Settings {
        Settings::for_project(&owner.root)
    }

    fn type_state(&self, owner: &Checkout) -> Result<TypeState, OrchestrationError> {
        let stored = Self::project_settings(owner)
            .get(PROJECT_TYPE_SETTING, Scope::Shared)
            .map_err(settings_error)?;
        let manifest = self.flow(owner)?.declarations()?;
        Ok(TypeState {
            current: ProjectType::effective(stored.as_ref(), manifest.enabled)?,
            declared: manifest.enabled,
            active_projects: manifest
                .targets
                .iter()
                .filter(|target| target.state == DeclarationState::Active)
                .count(),
        })
    }

    fn type_view(state: TypeState, changed: bool) -> ProjectTypeView {
        ProjectTypeView {
            project_type: state.current,
            declared: state.declared,
            active_projects: state.active_projects,
            changed,
        }
    }

    /// HS2-J3ZF27: whether the owner orchestrates now. A Software project has no
    /// orchestration: the scheduler places nothing and acting actions are refused,
    /// whatever its kept policy says. An unknown owner or unreadable type does not.
    pub(super) fn orchestrating(&self, owner_id: &str) -> Result<bool, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        Ok(self.type_state(&owner)?.current.orchestrates())
    }

    /// The project's type and how many projects its declaration holds. Reads two
    /// checked-in files; contacts nothing.
    pub fn project_type(&self, owner_id: &str) -> Result<ProjectTypeView, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        Ok(Self::type_view(self.type_state(&owner)?, false))
    }

    /// Switch the type. To Orchestration creates the empty declaration on first use;
    /// back to Software keeps every added project, and needs `keep_projects` to say so
    /// when there are any. A repeated request changes nothing.
    pub fn set_project_type(
        &self,
        owner_id: &str,
        request: SetProjectType,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ProjectTypeView, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let state = self.type_state(&owner)?;
        let change = plan_type_change(state, request.project_type, request.keep_projects)?;
        if change.create_declaration {
            // A concurrent first use wrote it meanwhile: that one is the declaration.
            match self.flow(&owner)?.replace(
                ReplaceTargets {
                    expected_revision: None,
                    targets: Vec::new(),
                },
                &observer::RuntimeObserver::new(self, &owner),
                tx,
            ) {
                Ok(_) | Err(OrchestrationError::Conflict) => {}
                Err(error) => return Err(error),
            }
        }
        if change.write_setting {
            Self::project_settings(&owner)
                .set(
                    PROJECT_TYPE_SETTING,
                    request.project_type.setting_value(),
                    Scope::Shared,
                )
                .map_err(settings_error)?;
        }
        let changed = change.write_setting || change.create_declaration;
        if change.write_setting {
            // HS2-J3ZF27: a running loop learns at once that it stops (to Software)
            // or that a kept automatic policy applies again.
            Self::scheduler_wake_at(&self.home);
        }
        if changed {
            self.audit_record(
                owner_id,
                AuditRecord {
                    at_ms: tx.clock.now_ms(),
                    actor_role: access::role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: None,
                    action: "project_type_set".into(),
                    operation_id: None,
                    outcome: AuditOutcome::Changed,
                    reason: Some(request.project_type.code().into()),
                    permission_id: None,
                    requested_by: None,
                },
            )?;
        }
        Ok(Self::type_view(self.type_state(&owner)?, changed))
    }

    /// Declare a target for an open checkout and bind it, in one step. Adding is a
    /// person's consent, so an AI actor is refused. Idempotent: a project already added
    /// and bound to the same checkout is returned unchanged, and a retry with the same
    /// `target_id` after a failed bind binds that target.
    /// HS2-92FJXM: the checkouts this server has open (registered, its directory
    /// present and its identity unchanged, as `remote-checkouts` reports them
    /// available), other than the orchestration project itself, each with whether it
    /// is already added here.
    pub fn hosted_project_choices(
        &self,
        owner_id: &str,
    ) -> Result<Vec<HostedProjectChoice>, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let flow = self.flow(&owner)?;
        let manifest = flow.declarations()?;
        let mut bound = Vec::new();
        for declaration in manifest
            .targets
            .iter()
            .filter(|target| target.state == DeclarationState::Active)
        {
            let reference = TargetRef {
                owner_checkout_id: owner.id.clone(),
                target_id: declaration.target_id.clone(),
            };
            if let Some(TargetDestination::Local { checkout_id, .. }) =
                flow.binding(&reference)?.destination
            {
                bound.push((checkout_id, declaration.target_id.clone()));
            }
        }
        let mut choices: Vec<HostedProjectChoice> = self
            .open_checkouts()?
            .into_iter()
            .filter(|checkout| checkout.id != owner.id)
            .map(|checkout| {
                let target_id = bound
                    .iter()
                    .find(|(id, _)| *id == checkout.id)
                    .map(|(_, target)| target.clone());
                // HS2-89YNGN: the same machine-local inspection the local Add a Site uses;
                // it binds nothing.
                let checkout_authority = self
                    .inspect(
                        owner_id,
                        hotsheet_ticketing::orchestration::InspectTarget {
                            destination: InspectDestination::Local {
                                checkout_id: checkout.id.clone(),
                            },
                        },
                    )
                    .ok()
                    .map(|inspection| inspection.destination.authority().to_owned());
                HostedProjectChoice {
                    name: checkout.alias.clone(),
                    added: target_id.is_some(),
                    target_id,
                    checkout_id: checkout.id,
                    checkout_authority,
                }
            })
            .collect();
        choices.sort_by(|a, b| (&a.name, &a.checkout_id).cmp(&(&b.name, &b.checkout_id)));
        Ok(choices)
    }

    /// The registered checkouts this server can serve now.
    fn open_checkouts(&self) -> Result<Vec<Checkout>, OrchestrationError> {
        Ok(self
            .checkouts
            .list()
            .map_err(checkout_error)?
            .into_iter()
            .filter(|checkout| {
                std::path::Path::new(&checkout.root).is_dir()
                    && self
                        .checkouts
                        .resolve(&checkout.id)
                        .is_ok_and(|resolved| resolved.id == checkout.id)
            })
            .collect())
    }

    /// HS2-92FJXM: add one of this server's open checkouts. It is the same workflow as
    /// [`Self::add_project`] with a local destination, so a repeat is the same project;
    /// a checkout this server does not have open is refused before anything is read.
    /// Reached from another machine only by a read-write device (the write fence).
    pub fn add_hosted_project(
        &self,
        owner_id: &str,
        request: AddHostedProject,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ProjectView, OrchestrationError> {
        access::person(actor)?;
        if !self
            .open_checkouts()?
            .iter()
            .any(|checkout| checkout.id == request.checkout_id)
        {
            return Err(OrchestrationError::Forbidden(CHECKOUT_NOT_OPEN));
        }
        self.add_project(
            owner_id,
            AddProject {
                destination: InspectDestination::Local {
                    checkout_id: request.checkout_id,
                },
                label: request.label,
                target_id: request.target_id,
                site_of: request.site_of,
            },
            actor,
            tx,
        )
    }

    pub fn add_project(
        &self,
        owner_id: &str,
        request: AddProject,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ProjectView, OrchestrationError> {
        access::person(actor)?;
        let owner = self.owner(owner_id)?;
        if self.type_state(&owner)?.current != ProjectType::Orchestration {
            return Err(OrchestrationError::Invalid(
                "set the project type to orchestration before adding projects",
            ));
        }
        let inspect = InspectTarget {
            destination: request.destination.clone(),
        };
        inspect.validate()?;
        if let InspectDestination::Local { checkout_id } = &request.destination
            && *checkout_id == owner.id
        {
            return Err(OrchestrationError::Invalid(
                "a project cannot orchestrate itself",
            ));
        }
        let flow = self.flow(&owner)?;
        let observer = observer::RuntimeObserver::new(self, &owner);
        let reference = |target_id: &str| TargetRef {
            owner_checkout_id: owner.id.clone(),
            target_id: target_id.into(),
        };
        // Already added and bound here: nothing to do.
        let manifest = flow.declarations()?;
        for declaration in manifest
            .targets
            .iter()
            .filter(|target| target.state == DeclarationState::Active)
        {
            let binding = flow.binding(&reference(&declaration.target_id))?;
            if binding
                .destination
                .as_ref()
                .is_some_and(|bound| same_place(bound, &request.destination))
            {
                return Ok(ProjectView {
                    target_id: declaration.target_id.clone(),
                    label: declaration.label.clone(),
                    added: false,
                    resolution: flow.resolve(&reference(&declaration.target_id), &observer, tx)?,
                });
            }
        }
        // Inspect before declaring, so an unreachable or unknown checkout adds nothing.
        let candidate = observer.inspect(inspect)?;
        // HS2-E9VVQF: a site of another ticket store is refused before it is declared.
        if let Some(project) = &request.site_of {
            flow.check_site_candidate(
                project,
                &candidate.destination,
                &candidate.projection.sources,
            )?;
        }
        let label = request
            .label
            .clone()
            .filter(|label| !label.trim().is_empty())
            .unwrap_or_else(|| candidate.destination.checkout_id().to_owned());
        let retained = request.target_id.as_ref().and_then(|id| {
            manifest
                .targets
                .iter()
                .find(|target| &target.target_id == id)
        });
        let (target_id, label) = match retained {
            Some(existing) if existing.state == DeclarationState::Removed => {
                return Err(OrchestrationError::Removed);
            }
            Some(existing) => (existing.target_id.clone(), existing.label.clone()),
            None => {
                let before: std::collections::BTreeSet<String> = manifest
                    .targets
                    .iter()
                    .map(|target| target.target_id.clone())
                    .collect();
                let added = flow.add(
                    AddTarget {
                        expected_revision: manifest.revision.clone(),
                        target_id: request.target_id.clone(),
                        label,
                        expected_repository_identity: None,
                        site_of: request.site_of.clone(),
                    },
                    &observer,
                    tx,
                )?;
                let declared = added
                    .targets
                    .iter()
                    .find(|target| match &request.target_id {
                        Some(id) => &target.target_id == id,
                        None => !before.contains(&target.target_id),
                    })
                    .ok_or(OrchestrationError::Corrupt)?;
                (declared.target_id.clone(), declared.label.clone())
            }
        };
        let target = reference(&target_id);
        let binding = flow.binding(&target)?;
        let manifest_revision = flow
            .declarations()?
            .revision
            .ok_or(OrchestrationError::Conflict)?;
        let resolution = flow.bind(
            BindTarget {
                target: target.clone(),
                expected_manifest_revision: manifest_revision,
                expected_binding_revision: binding.revision,
                previous_destination: binding.destination.clone(),
                destination: candidate.destination,
            },
            &observer,
            tx,
        )?;
        self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: Some(target_id.clone()),
                action: "project_add".into(),
                operation_id: None,
                outcome: AuditOutcome::Succeeded,
                reason: None,
                permission_id: None,
                requested_by: None,
            },
        )?;
        Ok(ProjectView {
            target_id,
            label,
            added: true,
            resolution,
        })
    }

    /// Stop orchestrating one project: remove this machine's binding, then tombstone
    /// the declaration (its id is never reused). Removing only reduces what the
    /// orchestrator can reach, so any identified or anonymous actor may do it; sessions
    /// already running there are not stopped by this. Repeating it changes nothing.
    /// HS2-0G80WD: stop orchestrating one project from another machine, mirroring
    /// [`Self::add_hosted_project`]: a person only (an AI actor is refused), reached by a
    /// read-write device through the write fence, and the same workflow as the local
    /// remove, so a repeat changes nothing and an unknown project is `target_not_found`.
    pub fn remove_hosted_project(
        &self,
        target: &TargetRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RemovedProject, OrchestrationError> {
        access::person(actor)?;
        self.remove_project(target, RemoveProject::default(), actor, tx)
    }

    pub fn remove_project(
        &self,
        target: &TargetRef,
        _request: RemoveProject,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RemovedProject, OrchestrationError> {
        let owner = self.owner(&target.owner_checkout_id)?;
        let flow = self.flow(&owner)?;
        let manifest = flow.declarations()?;
        let declaration = manifest
            .targets
            .iter()
            .find(|declared| declared.target_id == target.target_id)
            .ok_or(OrchestrationError::NotFound)?;
        let binding = flow.binding(target)?;
        let unbound = binding.destination.is_some();
        if unbound {
            flow.unbind(
                UnbindTarget {
                    target: target.clone(),
                    expected_binding_revision: binding.revision,
                    previous_destination: binding.destination,
                },
                tx,
            )?;
        }
        let removed = declaration.state == DeclarationState::Active;
        if removed {
            flow.remove(
                target,
                RemoveTarget {
                    expected_revision: manifest
                        .revision
                        .clone()
                        .ok_or(OrchestrationError::Corrupt)?,
                },
                &observer::RuntimeObserver::new(self, &owner),
                tx,
            )?;
        }
        if removed {
            // HS2-5675T5: the removed target's remembered rules can never answer again;
            // forget them (audited) now. The removal stands even if this cannot.
            let _ = self.forget_orphaned_permission_rules(tx.clock.now_ms());
        }
        if unbound || removed {
            self.audit_record(
                &owner.id,
                AuditRecord {
                    at_ms: tx.clock.now_ms(),
                    actor_role: access::role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: Some(target.target_id.clone()),
                    action: "project_remove".into(),
                    operation_id: None,
                    outcome: AuditOutcome::Succeeded,
                    reason: None,
                    permission_id: None,
                    requested_by: None,
                },
            )?;
        }
        Ok(RemovedProject {
            target_id: target.target_id.clone(),
            unbound,
            removed,
        })
    }
}

impl OrchestrationRuntime {
    /// HS2-SHHGC6: a digest of what Settings → Orchestration shows for the owner: its
    /// effective project type, its target declaration, its policy and this machine's
    /// binding of each declared target. A host compares it before and after a file
    /// change to announce a change a CLI process wrote without a server request.
    /// Times that every resolve renews (when a binding was last checked or confirmed)
    /// are left out, so reading the settings never changes the digest. Writes nothing
    /// and contacts nothing.
    pub fn orchestration_fingerprint(&self, owner_id: &str) -> Result<u64, OrchestrationError> {
        use std::hash::{Hash, Hasher};
        let owner = self.owner(owner_id)?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        // An unreadable type or policy is part of what shows; it hashes as its code.
        match self.type_state(&owner) {
            Ok(state) => (state.current.code(), state.declared).hash(&mut hasher),
            Err(error) => error.code().hash(&mut hasher),
        }
        let manifest = self.flow(&owner)?.declarations()?;
        serde_json::to_string(&manifest)
            .map_err(|_| OrchestrationError::Corrupt)?
            .hash(&mut hasher);
        match self.policy(owner_id) {
            Ok(view) => (view.enabled, view.revision).hash(&mut hasher),
            Err(error) => error.code().hash(&mut hasher),
        }
        for binding in self.bindings(owner_id)? {
            (
                binding.target.target_id,
                serde_json::to_string(&binding.destination).unwrap_or_default(),
                serde_json::to_string(&binding.last_failure).unwrap_or_default(),
                binding.confirmed_at_ms.is_some(),
            )
                .hash(&mut hasher);
        }
        Ok(hasher.finish())
    }

    /// HS2-SHHGC6: the directories whose files the fingerprint reads: the owner's
    /// `.hotsheet2` (project type, declaration, policy) and this machine's home
    /// (bindings). Each is watched without recursion.
    pub fn orchestration_watch_paths(
        &self,
        owner_id: &str,
    ) -> Result<Vec<PathBuf>, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        Ok(vec![
            Path::new(&owner.root).join(".hotsheet2"),
            self.home.clone(),
        ])
    }
}
