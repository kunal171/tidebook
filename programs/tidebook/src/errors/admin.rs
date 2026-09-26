//! Protocol governance and administrator authorization errors.

pub use super::TidebookError::{
    AdminAlreadyActive, AdminAlreadyDisabled, AdminDisabled, AdminMustBeDisabled,
    CannotModifySuperAdmin, InvalidAdmin, InvalidDeployer, UnauthorizedAdmin,
    UnauthorizedSuperAdmin,
};
