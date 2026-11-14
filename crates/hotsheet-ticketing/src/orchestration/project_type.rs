//! HS2-W27N1D: a project's type and what switching it does.
//!
//! The type is the shared, checked-in `project_type` setting of a code project
//! (`.hotsheet2/settings.json`). `software` is the default and means an ordinary
//! project. `orchestration` makes the project plan work across other projects: its
//! Orchestration settings and its target declaration (§26.2) apply. Switching is a
//! pure decision here; the runtime reads the current state, asks this table what to
//! do and then writes. Nothing is ever dropped: switching back to `software` keeps the
//! declaration, the bindings and the policy, so a later switch to `orchestration`
//! finds every project where it was.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::OrchestrationError;

/// The shared project settings key.
pub const PROJECT_TYPE_SETTING: &str = "project_type";

/// Refusal text when a switch to Software would leave added projects behind without
/// the caller saying it knows. Stable; clients match the `keep_projects` hint.
pub const PROJECTS_RETAINED: &str = "projects are still added; confirm keep_projects to keep them declared while the type is software";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectType {
    #[default]
    Software,
    Orchestration,
}

impl ProjectType {
    pub fn code(self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::Orchestration => "orchestration",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "software" => Some(Self::Software),
            "orchestration" => Some(Self::Orchestration),
            _ => None,
        }
    }

    /// The stored setting: absent (or null) is Software. A value this version does not
    /// know is refused rather than read as Software, so a newer type is never hidden.
    pub fn from_setting(value: Option<&Value>) -> Result<Self, OrchestrationError> {
        match value {
            None | Some(Value::Null) => Ok(Self::Software),
            Some(Value::String(text)) => {
                Self::parse(text).ok_or(OrchestrationError::Invalid("unknown project type"))
            }
            Some(_) => Err(OrchestrationError::Invalid("project type must be a string")),
        }
    }

    /// HS2-J3ZF27: the type in effect. A stored value wins. Without one, a project
    /// that already has a target declaration declared it before Project Type existed
    /// and stays an orchestration project; any other project is Software.
    pub fn effective(value: Option<&Value>, declared: bool) -> Result<Self, OrchestrationError> {
        match value {
            None | Some(Value::Null) if declared => Ok(Self::Orchestration),
            _ => Self::from_setting(value),
        }
    }

    /// HS2-J3ZF27: a Software project has no orchestration: nothing is placed or
    /// dispatched for it, whatever its kept policy says.
    pub fn orchestrates(self) -> bool {
        self == Self::Orchestration
    }

    pub fn setting_value(self) -> Value {
        Value::String(self.code().into())
    }
}

/// What the runtime observed before a switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeState {
    pub current: ProjectType,
    /// The target declaration file exists (it may hold no targets).
    pub declared: bool,
    /// Active (not removed) targets in the declaration.
    pub active_projects: usize,
}

/// What a switch writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeChange {
    /// The setting changes.
    pub write_setting: bool,
    /// The empty target declaration is created first (first use as Orchestration).
    pub create_declaration: bool,
    /// Added projects that stay declared while the type is Software.
    pub retained_projects: usize,
}

/// The decision table. Switching to the current type is a no-op apart from creating a
/// missing declaration for an Orchestration project, so repeating a request converges.
pub fn plan_type_change(
    state: TypeState,
    requested: ProjectType,
    keep_projects: bool,
) -> Result<TypeChange, OrchestrationError> {
    let retained = |to: ProjectType| match to {
        ProjectType::Software => state.active_projects,
        ProjectType::Orchestration => 0,
    };
    match (state.current, requested) {
        (from, to) if from == to => Ok(TypeChange {
            write_setting: false,
            create_declaration: to == ProjectType::Orchestration && !state.declared,
            retained_projects: retained(to),
        }),
        (_, ProjectType::Orchestration) => Ok(TypeChange {
            write_setting: true,
            create_declaration: !state.declared,
            retained_projects: 0,
        }),
        (_, ProjectType::Software) => {
            if state.active_projects > 0 && !keep_projects {
                return Err(OrchestrationError::Invalid(PROJECTS_RETAINED));
            }
            Ok(TypeChange {
                write_setting: true,
                create_declaration: false,
                retained_projects: state.active_projects,
            })
        }
    }
}

/// A request to change the type.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetProjectType {
    pub project_type: ProjectType,
    /// Required to switch to Software while projects are added; they stay declared.
    #[serde(default)]
    pub keep_projects: bool,
}

/// The type and what it governs, as every adapter reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTypeView {
    pub project_type: ProjectType,
    /// The target declaration exists.
    pub declared: bool,
    /// Active targets in the declaration, whatever the type.
    pub active_projects: usize,
    /// This request changed the setting or created the declaration.
    #[serde(default)]
    pub changed: bool,
}
