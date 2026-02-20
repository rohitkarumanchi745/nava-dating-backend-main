//! Authentication and authorization module
//!
//! Provides JWT authentication, RBAC authorization, and request signing.

pub mod jwt;
pub mod rbac;
pub mod signing;

pub use jwt::*;
pub use rbac::{
    AuthorizationError, Permission, RbacPolicy, RequirePermission, Role, UserContext,
};
pub use signing::{
    RequestSigner, RequestVerifier, SignedRequest, SigningConfig, SigningError,
};
