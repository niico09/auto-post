//! Manifest model, loading and validation.

mod loader;
mod model;
mod project;
mod validation;

pub use loader::{load_project, ManifestError};
pub use model::{
    ExtractSource, Extractor, HttpMethod, InputDef, RecoveryAction, RecoveryPolicy, RequestDef,
    Step, StepShapeError, StepTarget, Workflow,
};
pub use project::Project;
pub use validation::ValidationError;
