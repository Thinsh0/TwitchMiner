//! Coordination between TwitchMiner instances so one account is mined once.
//!
//! Each instance knows its peers (`engine.peers`) and periodically reads their
//! state through the API. A `secondary` instance puts an account in `standby` while a
//! reachable peer mines it, and takes it back after the peer has been gone for two
//! checks in a row (failover). A `primary` instance never yields; it raises a notice
//! when another instance mines the same account.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde_json::Value;
use tm_twitch::Net;

use crate::settings::{Peer, Role};
use crate::state::AccountStatus;

pub(crate) const CHECK_EVERY: Duration = Duration::from_secs(30);
/// Consecutive checks without the peer before a secondary takes an account back.
pub(crate) const TAKEOVER_AFTER: u32 = 2;

/// Accounts a peer is currently mining (empty if the peer is this very instance).
pub(crate) async fn running_on(
    net: &Net,
    peer: &Peer,
    own_instance: &str,
) -> Result<HashSet<String>, String> {
    let mut req = net
        .http
        .get(format!("{}/api/v1/state", peer.url.trim_end_matches('/')))
        .timeout(Duration::from_secs(8));
    if let Some(t) = &peer.token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    // Parsed loosely so peers running another version still interoperate.
    let state: Value = resp.json().await.map_err(|e| e.to_string())?;
    if state["instance_id"]
        .as_str()
        .is_some_and(|id| id == own_instance)
    {
        return Ok(HashSet::new());
    }
    Ok(state["accounts"]
        .as_object()
        .map(|accounts| {
            accounts
                .iter()
                .filter(|(_, a)| {
                    matches!(a["status"]["state"].as_str(), Some("running" | "starting"))
                })
                .map(|(id, _)| id.clone())
                .collect()
        })
        .unwrap_or_default())
}

/// Local account situation as seen by the coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Local {
    /// Starting, running or retrying: this instance is (trying to be) mining it.
    Active,
    Standby,
    /// Stopped, disabled or waiting for a login: nothing to coordinate.
    Inactive,
}

impl Local {
    pub fn of(status: Option<&AccountStatus>) -> Self {
        match status {
            Some(
                AccountStatus::Starting | AccountStatus::Running | AccountStatus::Error { .. },
            ) => Local::Active,
            Some(AccountStatus::Standby { .. }) => Local::Standby,
            _ => Local::Inactive,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Action {
    Standby { account: String, peer: String },
    Resume { account: String },
    Warn { account: String, peer: String },
    ClearWarn { account: String },
}

/// Pure decision step. `remote` maps account → label of a peer mining it;
/// `missing` counts consecutive checks without a peer for standby accounts.
pub(crate) fn decide(
    role: Role,
    local: &[(String, Local)],
    remote: &HashMap<String, String>,
    missing: &mut HashMap<String, u32>,
) -> Vec<Action> {
    let mut out = Vec::new();
    for (account, state) in local {
        let peer = remote.get(account);
        match (role, state, peer) {
            (Role::Secondary, Local::Active, Some(peer)) => {
                missing.remove(account);
                out.push(Action::Standby {
                    account: account.clone(),
                    peer: peer.clone(),
                });
            }
            (Role::Secondary, Local::Standby, Some(_)) => {
                missing.remove(account);
            }
            (Role::Secondary, Local::Standby, None) => {
                let n = missing.entry(account.clone()).or_insert(0);
                *n += 1;
                if *n >= TAKEOVER_AFTER {
                    missing.remove(account);
                    out.push(Action::Resume {
                        account: account.clone(),
                    });
                }
            }
            (Role::Primary, Local::Active, Some(peer)) => out.push(Action::Warn {
                account: account.clone(),
                peer: peer.clone(),
            }),
            (Role::Primary, Local::Standby, _) => out.push(Action::Resume {
                account: account.clone(),
            }),
            (Role::Primary, _, None) => out.push(Action::ClearWarn {
                account: account.clone(),
            }),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(a, p)| (a.to_string(), p.to_string()))
            .collect()
    }

    #[test]
    fn secondary_yields_then_takes_over_after_two_misses() {
        let mut missing = HashMap::new();
        let active = [("a".to_string(), Local::Active)];
        let acts = decide(
            Role::Secondary,
            &active,
            &remote(&[("a", "server")]),
            &mut missing,
        );
        assert_eq!(
            acts,
            vec![Action::Standby {
                account: "a".into(),
                peer: "server".into()
            }]
        );

        let standby = [("a".to_string(), Local::Standby)];
        assert!(
            decide(
                Role::Secondary,
                &standby,
                &remote(&[("a", "server")]),
                &mut missing
            )
            .is_empty()
        );
        // Peer gone: first miss waits, second takes over.
        assert!(decide(Role::Secondary, &standby, &HashMap::new(), &mut missing).is_empty());
        assert_eq!(
            decide(Role::Secondary, &standby, &HashMap::new(), &mut missing),
            vec![Action::Resume {
                account: "a".into()
            }]
        );
    }

    #[test]
    fn a_blip_does_not_cause_takeover() {
        let mut missing = HashMap::new();
        let standby = [("a".to_string(), Local::Standby)];
        decide(Role::Secondary, &standby, &HashMap::new(), &mut missing);
        decide(
            Role::Secondary,
            &standby,
            &remote(&[("a", "server")]),
            &mut missing,
        );
        assert!(decide(Role::Secondary, &standby, &HashMap::new(), &mut missing).is_empty());
    }

    #[test]
    fn primary_warns_and_never_yields() {
        let mut missing = HashMap::new();
        let active = [
            ("a".to_string(), Local::Active),
            ("b".to_string(), Local::Active),
        ];
        let acts = decide(
            Role::Primary,
            &active,
            &remote(&[("a", "phone")]),
            &mut missing,
        );
        assert_eq!(
            acts,
            vec![
                Action::Warn {
                    account: "a".into(),
                    peer: "phone".into()
                },
                Action::ClearWarn {
                    account: "b".into()
                },
            ]
        );
    }

    #[test]
    fn inactive_accounts_are_left_alone_by_secondaries() {
        let mut missing = HashMap::new();
        let inactive = [("a".to_string(), Local::Inactive)];
        assert!(
            decide(
                Role::Secondary,
                &inactive,
                &remote(&[("a", "server")]),
                &mut missing
            )
            .is_empty()
        );
    }
}
