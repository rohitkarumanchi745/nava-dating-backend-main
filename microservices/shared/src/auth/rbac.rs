//! Role-Based Access Control (RBAC) system
//!
//! Provides fine-grained permissions for users, services, and admins.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// User roles in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Regular user
    User,
    /// Premium subscriber
    Premium,
    /// Verified user (selfie/student verified)
    Verified,
    /// Ambassador
    Ambassador,
    /// Moderator (content moderation)
    Moderator,
    /// Admin (full access)
    Admin,
    /// Service account (inter-service communication)
    Service,
    /// Super admin (system configuration)
    SuperAdmin,
}

impl Role {
    /// Get role hierarchy level (higher = more privileges)
    pub fn level(&self) -> u8 {
        match self {
            Role::User => 1,
            Role::Verified => 2,
            Role::Premium => 3,
            Role::Ambassador => 4,
            Role::Moderator => 5,
            Role::Admin => 6,
            Role::Service => 7,
            Role::SuperAdmin => 8,
        }
    }

    /// Check if this role includes another role's permissions
    pub fn includes(&self, other: &Role) -> bool {
        self.level() >= other.level()
    }
}

/// Permissions for specific actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    // User actions
    ReadProfile,
    WriteProfile,
    DeleteProfile,

    // Matching
    ViewDiscover,
    SendLike,
    UnlimitedLikes,
    ViewLikers,
    SuperLike,

    // Chat
    SendMessage,
    ReadReceipts,
    VoiceMessages,

    // Payments
    ViewPlans,
    Subscribe,
    ManageSubscription,
    ViewPaymentHistory,
    ProcessRefund,

    // Ambassador
    ViewReferrals,
    GenerateReferralCode,
    ViewCommissions,
    WithdrawCommissions,

    // Moderation
    ViewReports,
    ModerateContent,
    BanUser,
    UnbanUser,

    // Admin
    ViewAllUsers,
    EditAnyUser,
    ViewAnalytics,
    ManageAmbassadors,
    ManageAds,
    SystemConfig,

    // Service
    InternalApi,
    PublishEvents,
    ConsumeEvents,
}

/// RBAC policy defining role-permission mappings
pub struct RbacPolicy {
    role_permissions: HashMap<Role, HashSet<Permission>>,
}

impl Default for RbacPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl RbacPolicy {
    /// Create a new RBAC policy with default mappings
    pub fn new() -> Self {
        let mut policy = Self {
            role_permissions: HashMap::new(),
        };

        // User permissions
        policy.grant(Role::User, &[
            Permission::ReadProfile,
            Permission::WriteProfile,
            Permission::ViewDiscover,
            Permission::SendLike,
            Permission::SendMessage,
            Permission::ViewPlans,
            Permission::Subscribe,
        ]);

        // Verified user permissions (includes User)
        policy.grant(Role::Verified, &[
            Permission::ReadProfile,
            Permission::WriteProfile,
            Permission::ViewDiscover,
            Permission::SendLike,
            Permission::SendMessage,
            Permission::ViewPlans,
            Permission::Subscribe,
        ]);

        // Premium permissions (includes Verified)
        policy.grant(Role::Premium, &[
            Permission::ReadProfile,
            Permission::WriteProfile,
            Permission::ViewDiscover,
            Permission::SendLike,
            Permission::UnlimitedLikes,
            Permission::ViewLikers,
            Permission::SuperLike,
            Permission::SendMessage,
            Permission::ReadReceipts,
            Permission::VoiceMessages,
            Permission::ViewPlans,
            Permission::Subscribe,
            Permission::ManageSubscription,
            Permission::ViewPaymentHistory,
        ]);

        // Ambassador permissions
        policy.grant(Role::Ambassador, &[
            Permission::ReadProfile,
            Permission::WriteProfile,
            Permission::ViewDiscover,
            Permission::SendLike,
            Permission::SendMessage,
            Permission::ViewPlans,
            Permission::Subscribe,
            Permission::ViewReferrals,
            Permission::GenerateReferralCode,
            Permission::ViewCommissions,
            Permission::WithdrawCommissions,
        ]);

        // Moderator permissions
        policy.grant(Role::Moderator, &[
            Permission::ReadProfile,
            Permission::ViewReports,
            Permission::ModerateContent,
            Permission::BanUser,
            Permission::UnbanUser,
        ]);

        // Admin permissions (includes Moderator)
        policy.grant(Role::Admin, &[
            Permission::ReadProfile,
            Permission::WriteProfile,
            Permission::ViewReports,
            Permission::ModerateContent,
            Permission::BanUser,
            Permission::UnbanUser,
            Permission::ViewAllUsers,
            Permission::EditAnyUser,
            Permission::ViewAnalytics,
            Permission::ManageAmbassadors,
            Permission::ManageAds,
            Permission::ProcessRefund,
        ]);

        // Service account permissions
        policy.grant(Role::Service, &[
            Permission::InternalApi,
            Permission::PublishEvents,
            Permission::ConsumeEvents,
            Permission::ViewAllUsers,
        ]);

        // Super admin - all permissions
        policy.grant(Role::SuperAdmin, &[
            Permission::ReadProfile,
            Permission::WriteProfile,
            Permission::DeleteProfile,
            Permission::ViewDiscover,
            Permission::SendLike,
            Permission::UnlimitedLikes,
            Permission::ViewLikers,
            Permission::SuperLike,
            Permission::SendMessage,
            Permission::ReadReceipts,
            Permission::VoiceMessages,
            Permission::ViewPlans,
            Permission::Subscribe,
            Permission::ManageSubscription,
            Permission::ViewPaymentHistory,
            Permission::ProcessRefund,
            Permission::ViewReferrals,
            Permission::GenerateReferralCode,
            Permission::ViewCommissions,
            Permission::WithdrawCommissions,
            Permission::ViewReports,
            Permission::ModerateContent,
            Permission::BanUser,
            Permission::UnbanUser,
            Permission::ViewAllUsers,
            Permission::EditAnyUser,
            Permission::ViewAnalytics,
            Permission::ManageAmbassadors,
            Permission::ManageAds,
            Permission::SystemConfig,
            Permission::InternalApi,
            Permission::PublishEvents,
            Permission::ConsumeEvents,
        ]);

        policy
    }

    /// Grant permissions to a role
    pub fn grant(&mut self, role: Role, permissions: &[Permission]) {
        let perms = self.role_permissions.entry(role).or_insert_with(HashSet::new);
        for perm in permissions {
            perms.insert(*perm);
        }
    }

    /// Check if a role has a specific permission
    pub fn has_permission(&self, role: &Role, permission: &Permission) -> bool {
        self.role_permissions
            .get(role)
            .map(|perms| perms.contains(permission))
            .unwrap_or(false)
    }

    /// Check if any of the roles has a specific permission
    pub fn any_has_permission(&self, roles: &[Role], permission: &Permission) -> bool {
        roles.iter().any(|role| self.has_permission(role, permission))
    }

    /// Get all permissions for a role
    pub fn get_permissions(&self, role: &Role) -> Vec<Permission> {
        self.role_permissions
            .get(role)
            .map(|perms| perms.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// User context with roles and permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserContext {
    pub user_id: i32,
    pub roles: Vec<Role>,
    pub is_verified: bool,
    pub is_premium: bool,
    pub is_ambassador: bool,
}

impl UserContext {
    /// Check if user has a specific permission
    pub fn has_permission(&self, permission: &Permission, policy: &RbacPolicy) -> bool {
        policy.any_has_permission(&self.roles, permission)
    }

    /// Check if user has a specific role
    pub fn has_role(&self, role: &Role) -> bool {
        self.roles.contains(role)
    }

    /// Check if user has at least the given role level
    pub fn has_role_level(&self, min_role: &Role) -> bool {
        self.roles.iter().any(|r| r.includes(min_role))
    }
}

/// Authorization guard for Axum handlers
#[derive(Clone)]
pub struct RequirePermission {
    pub permission: Permission,
    pub policy: std::sync::Arc<RbacPolicy>,
}

impl RequirePermission {
    pub fn new(permission: Permission) -> Self {
        Self {
            permission,
            policy: std::sync::Arc::new(RbacPolicy::new()),
        }
    }

    pub fn check(&self, context: &UserContext) -> Result<(), AuthorizationError> {
        if context.has_permission(&self.permission, &self.policy) {
            Ok(())
        } else {
            Err(AuthorizationError::InsufficientPermissions {
                required: self.permission,
                user_roles: context.roles.clone(),
            })
        }
    }
}

/// Authorization errors
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("Insufficient permissions: required {required:?}, user has roles {user_roles:?}")]
    InsufficientPermissions {
        required: Permission,
        user_roles: Vec<Role>,
    },

    #[error("Role not found: {0:?}")]
    RoleNotFound(Role),

    #[error("User not authenticated")]
    NotAuthenticated,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_hierarchy() {
        assert!(Role::Admin.includes(&Role::Moderator));
        assert!(Role::SuperAdmin.includes(&Role::Admin));
        assert!(!Role::User.includes(&Role::Premium));
    }

    #[test]
    fn test_user_permissions() {
        let policy = RbacPolicy::new();

        assert!(policy.has_permission(&Role::User, &Permission::ReadProfile));
        assert!(!policy.has_permission(&Role::User, &Permission::UnlimitedLikes));
        assert!(policy.has_permission(&Role::Premium, &Permission::UnlimitedLikes));
    }

    #[test]
    fn test_admin_permissions() {
        let policy = RbacPolicy::new();

        assert!(policy.has_permission(&Role::Admin, &Permission::ViewAllUsers));
        assert!(policy.has_permission(&Role::Admin, &Permission::BanUser));
        assert!(!policy.has_permission(&Role::Admin, &Permission::SystemConfig));
        assert!(policy.has_permission(&Role::SuperAdmin, &Permission::SystemConfig));
    }
}
