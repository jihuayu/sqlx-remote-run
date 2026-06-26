use std::time::Duration;

use ipnet::IpNet;

use crate::sql_kind::StatementKind;

/// Authorization and network restrictions for remote SQL requests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthConfig {
    /// Static bearer token expected in the request context.
    ///
    /// When this is `None`, bearer token validation is disabled. Use this only
    /// for trusted internal transports or tests.
    pub bearer_token: Option<String>,
    /// Allowed client networks.
    ///
    /// An empty list disables IP filtering. When non-empty, requests without a
    /// peer IP are rejected.
    pub allowed_ips: Vec<IpNet>,
}

impl AuthConfig {
    /// Creates an auth config that requires a bearer token.
    #[must_use]
    pub fn bearer_token(token: impl Into<String>) -> Self {
        Self {
            bearer_token: Some(token.into()),
            allowed_ips: Vec::new(),
        }
    }

    /// Adds one allowed IP network.
    #[must_use]
    pub fn with_allowed_ip(mut self, network: IpNet) -> Self {
        self.allowed_ips.push(network);
        self
    }

    /// Adds multiple allowed IP networks.
    #[must_use]
    pub fn with_allowed_ips(mut self, networks: impl IntoIterator<Item = IpNet>) -> Self {
        self.allowed_ips.extend(networks);
        self
    }
}

/// Permission tier for accepted SQL statements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    /// Allows read-only query statements.
    ReadOnly,
    /// Allows read and DML write statements.
    ReadWrite,
    /// Allows read, DML, and administrative statements such as DDL and PRAGMA.
    Admin,
}

impl Permission {
    /// Returns true when this permission allows a statement kind.
    #[must_use]
    pub fn allows(self, kind: StatementKind) -> bool {
        matches!(
            (self, kind),
            (Self::Admin, _)
                | (Self::ReadWrite, StatementKind::Read | StatementKind::Write)
                | (Self::ReadOnly, StatementKind::Read)
        )
    }
}

/// Execution limits applied before and during SQL execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionLimits {
    /// Maximum SQL request size in bytes.
    pub max_sql_bytes: usize,
    /// Maximum number of bound parameters.
    pub max_params: usize,
    /// Maximum number of rows returned by a query.
    pub max_rows: usize,
    /// Maximum time allowed for one execution.
    pub timeout: Duration,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_sql_bytes: 64 * 1024,
            max_params: 256,
            max_rows: 1_000,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Complete remote SQL service configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteSqlConfig {
    /// Authorization and IP filtering configuration.
    pub auth: AuthConfig,
    /// SQL permission tier.
    pub permission: Permission,
    /// Request and execution limits.
    pub limits: ExecutionLimits,
}

impl RemoteSqlConfig {
    /// Creates a config with explicit auth and permission and default limits.
    #[must_use]
    pub fn new(auth: AuthConfig, permission: Permission) -> Self {
        Self {
            auth,
            permission,
            limits: ExecutionLimits::default(),
        }
    }

    /// Replaces the default execution limits.
    #[must_use]
    pub fn with_limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }
}

impl Default for RemoteSqlConfig {
    fn default() -> Self {
        Self {
            auth: AuthConfig::default(),
            permission: Permission::ReadOnly,
            limits: ExecutionLimits::default(),
        }
    }
}
