use std::net::IpAddr;

use subtle::ConstantTimeEq;

use crate::{AuthConfig, RemoteSqlError};

pub(crate) fn validate_auth(
    config: &AuthConfig,
    bearer_token: Option<&str>,
    peer_ip: Option<IpAddr>,
) -> Result<(), RemoteSqlError> {
    validate_bearer_token(config, bearer_token)?;
    validate_peer_ip(config, peer_ip)
}

fn validate_bearer_token(
    config: &AuthConfig,
    bearer_token: Option<&str>,
) -> Result<(), RemoteSqlError> {
    let Some(expected) = config.bearer_token.as_deref() else {
        return Ok(());
    };

    let Some(actual) = bearer_token else {
        return Err(RemoteSqlError::Unauthorized);
    };

    let valid = expected.as_bytes().ct_eq(actual.as_bytes()).into();
    if valid {
        Ok(())
    } else {
        Err(RemoteSqlError::Unauthorized)
    }
}

fn validate_peer_ip(config: &AuthConfig, peer_ip: Option<IpAddr>) -> Result<(), RemoteSqlError> {
    if config.allowed_ips.is_empty() {
        return Ok(());
    }

    let Some(peer_ip) = peer_ip else {
        return Err(RemoteSqlError::MissingPeerIp);
    };

    if config
        .allowed_ips
        .iter()
        .any(|allowed_network| allowed_network.contains(&peer_ip))
    {
        Ok(())
    } else {
        Err(RemoteSqlError::ForbiddenIp)
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use ipnet::IpNet;

    use super::*;

    #[test]
    fn validate_auth_accepts_matching_bearer_token() {
        let config = AuthConfig::bearer_token("secret");

        let result = validate_auth(&config, Some("secret"), None);

        assert!(result.is_ok(), "auth error: {:?}", result.err());
    }

    #[test]
    fn validate_auth_rejects_missing_bearer_token() {
        let config = AuthConfig::bearer_token("secret");

        let error = validate_auth(&config, None, None).unwrap_err();

        assert!(matches!(error, RemoteSqlError::Unauthorized));
    }

    #[test]
    fn validate_auth_accepts_allowed_peer_ip() {
        let allowed_network: IpNet = "127.0.0.0/8".parse().unwrap();
        let config = AuthConfig::default().with_allowed_ip(allowed_network);

        let result = validate_auth(&config, None, Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));

        assert!(result.is_ok(), "auth error: {:?}", result.err());
    }

    #[test]
    fn validate_auth_rejects_missing_peer_ip_when_allowlist_is_configured() {
        let allowed_network: IpNet = "127.0.0.0/8".parse().unwrap();
        let config = AuthConfig::default().with_allowed_ip(allowed_network);

        let error = validate_auth(&config, None, None).unwrap_err();

        assert!(matches!(error, RemoteSqlError::MissingPeerIp));
    }
}
