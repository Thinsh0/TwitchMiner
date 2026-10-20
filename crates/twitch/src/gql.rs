//! Authenticated GQL client: persisted operations from the manifest, raw queries,
//! batching, retry with backoff and concurrency limiting.

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::auth::Credentials;
use crate::error::{Error, Result, is_transient_gql_message};
use crate::manifest::{SharedManifest, merge_json};
use crate::net::{Backoff, Net};

const GQL_URL: &str = "https://gql.twitch.tv/gql";
const MAX_BATCH: usize = 30;
const MAX_ATTEMPTS: u32 = 4;
const MAX_CONCURRENCY: usize = 6;

#[derive(Clone)]
pub struct Gql {
    inner: Arc<Inner>,
}

struct Inner {
    net: Net,
    manifest: SharedManifest,
    creds: Arc<ArcSwap<Credentials>>,
    session_id: String,
    user_agent_index: usize,
    limiter: Semaphore,
}

impl Gql {
    pub fn new(net: Net, manifest: SharedManifest, creds: Arc<ArcSwap<Credentials>>) -> Self {
        let session_id = format!("{:016x}", rand::random::<u64>());
        Self {
            inner: Arc::new(Inner {
                net,
                manifest,
                creds,
                session_id,
                user_agent_index: rand::random::<u32>() as usize,
                limiter: Semaphore::new(MAX_CONCURRENCY),
            }),
        }
    }

    pub fn net(&self) -> &Net {
        &self.inner.net
    }

    pub fn manifest(&self) -> SharedManifest {
        self.inner.manifest.clone()
    }

    pub fn credentials(&self) -> Arc<Credentials> {
        self.inner.creds.load_full()
    }

    pub fn user_id(&self) -> String {
        self.inner.creds.load().user_id.clone()
    }

    /// User agent of the client profile this account's token belongs to.
    pub fn user_agent(&self) -> String {
        let m = self.inner.manifest.load();
        let creds = self.inner.creds.load();
        let profile = m
            .profile(&creds.profile)
            .or_else(|| m.profile(&m.default_profile));
        profile
            .map(|p| p.user_agents[self.inner.user_agent_index % p.user_agents.len()].clone())
            .unwrap_or_else(|| m.web_user_agent.clone())
    }

    /// Runs a persisted operation declared in the manifest and returns its `data`.
    pub async fn op(&self, key: &str, variables: Value) -> Result<Value> {
        let body = self.build_op(key, variables)?;
        let mut out = self
            .send(Value::Array(vec![body]), &[key.to_string()])
            .await?;
        out.pop()
            .unwrap_or_else(|| Err(Error::decode("empty batch response")))
    }

    /// Runs several persisted operations, batched; results keep the input order.
    pub async fn op_batch(&self, ops: Vec<(&str, Value)>) -> Result<Vec<Result<Value>>> {
        let mut results = Vec::with_capacity(ops.len());
        let mut bodies = Vec::new();
        let mut names = Vec::new();
        for (key, vars) in ops {
            bodies.push(self.build_op(key, vars)?);
            names.push(key.to_string());
        }
        for (chunk, chunk_names) in bodies.chunks(MAX_BATCH).zip(names.chunks(MAX_BATCH)) {
            results.extend(self.send(Value::Array(chunk.to_vec()), chunk_names).await?);
        }
        Ok(results)
    }

    /// Runs a raw (non-persisted) query. Only some client profiles allow it.
    pub async fn raw(&self, query: &str, variables: Value) -> Result<Value> {
        let body = json!({ "query": query, "variables": variables });
        let mut out = self
            .send(Value::Array(vec![body]), &["raw".to_string()])
            .await?;
        out.pop()
            .unwrap_or_else(|| Err(Error::decode("empty batch response")))
    }

    pub fn raw_queries_allowed(&self) -> bool {
        let m = self.inner.manifest.load();
        let creds = self.inner.creds.load();
        m.profile(&creds.profile).is_some_and(|p| p.raw_queries)
    }

    fn build_op(&self, key: &str, variables: Value) -> Result<Value> {
        let m = self.inner.manifest.load();
        let op = m.operation(key)?;
        let mut vars = if op.variables.is_null() {
            json!({})
        } else {
            op.variables.clone()
        };
        merge_json(&mut vars, &variables);
        Ok(json!({
            "operationName": op.name,
            "variables": vars,
            "extensions": { "persistedQuery": { "version": 1, "sha256Hash": op.hash } }
        }))
    }

    /// Sends one batch, retrying transient failures of the whole batch.
    async fn send(&self, body: Value, names: &[String]) -> Result<Vec<Result<Value>>> {
        let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(20));
        loop {
            let attempt = self.send_once(&body, names).await;
            let retry = match &attempt {
                Err(e) => e.is_transient(),
                Ok(items) => items
                    .iter()
                    .all(|r| matches!(r, Err(e) if e.is_transient())),
            };
            if !retry || backoff.attempts() + 1 >= MAX_ATTEMPTS {
                return attempt;
            }
            let delay = backoff.next_delay();
            tracing::debug!(?names, ?delay, "retrying GQL batch");
            tokio::time::sleep(delay).await;
        }
    }

    async fn send_once(&self, body: &Value, names: &[String]) -> Result<Vec<Result<Value>>> {
        let _permit = self
            .inner
            .limiter
            .acquire()
            .await
            .map_err(|_| Error::Cancelled)?;
        let (client_id, client_url) = {
            let m = self.inner.manifest.load();
            let creds = self.inner.creds.load();
            let p = m
                .profile(&creds.profile)
                .or_else(|| m.profile(&m.default_profile))
                .ok_or_else(|| Error::decode("no client profile"))?;
            (p.client_id.clone(), p.client_url.clone())
        };
        let creds = self.inner.creds.load_full();
        let resp = self
            .inner
            .net
            .http
            .post(GQL_URL)
            .header("Client-Id", client_id)
            .header("Authorization", format!("OAuth {}", creds.access_token))
            .header("X-Device-Id", &creds.device_id)
            .header("Client-Session-Id", &self.inner.session_id)
            .header("User-Agent", self.user_agent())
            .header("Origin", &client_url)
            .header("Referer", &client_url)
            .header("Accept-Language", "en-US")
            .json(body)
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status == 401 {
            return Err(Error::Unauthorized);
        }
        if status >= 400 {
            return Err(Error::Http {
                status,
                url: GQL_URL.into(),
            });
        }
        let value: Value = resp.json().await?;
        let items = match value {
            Value::Array(items) => items,
            Value::Object(ref o) if o.contains_key("error") => {
                let msg = o.get("message").and_then(Value::as_str).unwrap_or_default();
                if o.get("status").and_then(Value::as_u64) == Some(401) || msg.contains("OAuth") {
                    return Err(Error::Unauthorized);
                }
                return Err(Error::Gql {
                    op: names.join(","),
                    message: msg.to_string(),
                });
            }
            other => vec![other],
        };
        Ok(items
            .into_iter()
            .zip(names)
            .map(|(item, name)| interpret(item, name))
            .collect())
    }
}

/// Turns one GQL response object into `data` or an error.
///
/// Partial errors (some field failed, `data` still present) are tolerated: the failed
/// field is `null` and callers already treat missing data as "unknown".
fn interpret(mut item: Value, name: &str) -> Result<Value> {
    let errors: Vec<String> = item
        .get("errors")
        .and_then(Value::as_array)
        .map(|errs| {
            errs.iter()
                .filter_map(|e| e.get("message").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if errors.iter().any(|m| m == "PersistedQueryNotFound") {
        return Err(Error::PersistedQueryNotFound(name.to_string()));
    }
    let data = item.get_mut("data").map(Value::take).unwrap_or(Value::Null);
    if !errors.is_empty() {
        let has_data = data
            .as_object()
            .is_some_and(|o| o.values().any(|v| !v.is_null()));
        if !has_data {
            let message = errors.join("; ");
            if errors
                .iter()
                .any(|m| m.contains("unauthorized") || m.contains("Unauthorized"))
            {
                return Err(Error::Unauthorized);
            }
            return Err(Error::Gql {
                op: name.to_string(),
                message,
            });
        }
        if errors.iter().any(|m| !is_transient_gql_message(m)) {
            tracing::debug!(op = name, ?errors, "partial GQL errors");
        }
    }
    if data.is_null() {
        return Err(Error::Gql {
            op: name.to_string(),
            message: "null data".into(),
        });
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_errors_keep_data() {
        let v = json!({"errors":[{"message":"service error"}],"data":{"a":1,"b":null}});
        assert_eq!(interpret(v, "x").unwrap(), json!({"a":1,"b":null}));
    }

    #[test]
    fn transient_without_data_is_transient() {
        let v = json!({"errors":[{"message":"service timeout"}],"data":null});
        let e = interpret(v, "x").unwrap_err();
        assert!(e.is_transient());
    }

    #[test]
    fn persisted_not_found() {
        let v = json!({"errors":[{"message":"PersistedQueryNotFound"}]});
        assert!(matches!(interpret(v, "Inv"), Err(Error::PersistedQueryNotFound(n)) if n == "Inv"));
    }
}
