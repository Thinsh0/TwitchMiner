//! Typed Twitch operations on top of [`Gql`].

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::gql::Gql;
use crate::models::*;

const USERS_QUERY: &str = "query($logins:[String!],$ids:[ID!]){users(logins:$logins,ids:$ids){\
id login displayName profileImageURL(width:70) \
stream{id title viewersCount createdAt game{id name displayName slug boxArtURL(width:144,height:192)}}}}";

impl Gql {
    /// Looks channels up by login (results keep input order; `None` = unknown login).
    pub async fn users_by_login(&self, logins: &[String]) -> Result<Vec<Option<UserInfo>>> {
        self.users(logins, "logins").await
    }

    /// Looks channels up by id (results keep input order).
    pub async fn users_by_id(&self, ids: &[String]) -> Result<Vec<Option<UserInfo>>> {
        self.users(ids, "ids").await
    }

    async fn users(&self, keys: &[String], field: &str) -> Result<Vec<Option<UserInfo>>> {
        let mut out = Vec::with_capacity(keys.len());
        if self.raw_queries_allowed() {
            for chunk in keys.chunks(100) {
                let data = self.raw(USERS_QUERY, json!({ field: chunk })).await?;
                let users = data
                    .get("users")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                out.extend((0..chunk.len()).map(|i| users.get(i).and_then(parse_user)));
            }
            return Ok(out);
        }
        // Persisted fallback: only works by login.
        if field != "logins" {
            return Err(Error::UnknownOperation(
                "users by id needs raw queries".into(),
            ));
        }
        let ops = keys
            .iter()
            .map(|l| ("StreamInfo", json!({ "channel": l })))
            .collect();
        for r in self.op_batch(ops).await? {
            out.push(r.ok().and_then(|d| d.get("user").and_then(parse_user)));
        }
        Ok(out)
    }

    pub async fn channel_points_context(&self, login: &str) -> Result<PointsContext> {
        let d = self
            .op("ChannelPointsContext", json!({ "channelLogin": login }))
            .await?;
        let channel = d
            .pointer("/community/channel")
            .ok_or_else(|| Error::decode("channel points context missing"))?;
        let cp = channel
            .pointer("/self/communityPoints")
            .cloned()
            .unwrap_or(Value::Null);
        let multiplier = arr(&cp, "activeMultipliers")
            .filter_map(|m| m.get("factor").and_then(Value::as_f64))
            .sum::<f64>();
        Ok(PointsContext {
            channel_id: s(channel, "id").unwrap_or_default(),
            balance: u(&cp, "balance").unwrap_or(0),
            available_claim: cp.get("availableClaim").and_then(|c| s(c, "id")),
            multiplier: 1.0 + multiplier,
        })
    }

    pub async fn claim_points(&self, channel_id: &str, claim_id: &str) -> Result<()> {
        self.op(
            "ClaimCommunityPoints",
            json!({"input": {"channelID": channel_id, "claimID": claim_id}}),
        )
        .await
        .map(drop)
    }

    pub async fn join_raid(&self, raid_id: &str) -> Result<()> {
        self.op("JoinRaid", json!({"input": {"raidID": raid_id}}))
            .await
            .map(drop)
    }

    pub async fn claim_moment(&self, moment_id: &str) -> Result<()> {
        self.op("ClaimMoment", json!({"input": {"momentID": moment_id}}))
            .await
            .map(drop)
    }

    /// In-progress campaigns (with personal progress) and already-claimed benefits.
    pub async fn inventory(&self) -> Result<(Vec<Campaign>, Vec<ClaimedBenefit>)> {
        let d = self.op("Inventory", json!({})).await?;
        let inv = d
            .pointer("/currentUser/inventory")
            .cloned()
            .unwrap_or(Value::Null);
        let campaigns = arr(&inv, "dropCampaignsInProgress")
            .filter_map(parse_campaign)
            .collect();
        let claimed = arr(&inv, "gameEventDrops")
            .filter_map(|b| {
                Some(ClaimedBenefit {
                    id: s(b, "id")?,
                    last_awarded_at: date(b, "lastAwardedAt"),
                })
            })
            .collect();
        Ok((campaigns, claimed))
    }

    /// All campaigns visible on the drops dashboard. `None` when Twitch hides it
    /// (observed for brand new accounts).
    pub async fn dashboard(&self) -> Result<Option<Vec<Campaign>>> {
        let d = self.op("Campaigns", json!({})).await?;
        Ok(d.pointer("/currentUser/dropCampaigns")
            .and_then(Value::as_array)
            .map(|cs| cs.iter().filter_map(parse_campaign).collect()))
    }

    pub async fn campaign_details(&self, ids: &[String]) -> Result<Vec<Option<Campaign>>> {
        let user_id = self.user_id();
        let ops = ids
            .iter()
            .map(|id| {
                (
                    "CampaignDetails",
                    json!({ "channelLogin": user_id, "dropID": id }),
                )
            })
            .collect();
        Ok(self
            .op_batch(ops)
            .await?
            .into_iter()
            .map(|r| {
                r.ok()
                    .and_then(|d| d.pointer("/user/dropCampaign").and_then(parse_campaign))
            })
            .collect())
    }

    /// Campaigns a set of channels currently participate in (same order as input).
    pub async fn available_drops(&self, channel_ids: &[String]) -> Result<Vec<Vec<Campaign>>> {
        let ops = channel_ids
            .iter()
            .map(|id| ("AvailableDrops", json!({ "channelID": id })))
            .collect();
        Ok(self
            .op_batch(ops)
            .await?
            .into_iter()
            .map(|r| {
                r.ok()
                    .and_then(|d| {
                        d.pointer("/channel/viewerDropCampaigns")
                            .and_then(Value::as_array)
                            .cloned()
                    })
                    .map(|cs| cs.iter().filter_map(parse_campaign).collect())
                    .unwrap_or_default()
            })
            .collect())
    }

    /// Live channels of a game, by viewer count.
    pub async fn game_directory(
        &self,
        slug: &str,
        limit: u32,
        drops_only: bool,
    ) -> Result<Vec<UserInfo>> {
        let filters: Vec<&str> = if drops_only {
            vec!["DROPS_ENABLED"]
        } else {
            vec![]
        };
        let d = self
            .op(
                "GameDirectory",
                json!({ "slug": slug, "limit": limit, "options": { "systemFilters": filters } }),
            )
            .await?;
        let game = d.get("game").and_then(parse_game);
        let edges = d
            .pointer("/game/streams/edges")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(edges
            .iter()
            .filter_map(|e| e.get("node"))
            .filter_map(|n| {
                let b = n.get("broadcaster")?;
                let mut user = parse_user(b)?;
                user.stream = Some(StreamInfo {
                    id: s(n, "id").unwrap_or_default(),
                    title: s(n, "title").unwrap_or_default(),
                    game: n.get("game").and_then(parse_game).or_else(|| game.clone()),
                    viewers: u(n, "viewersCount").unwrap_or(0),
                    started_at: None,
                });
                Some(user)
            })
            .collect())
    }

    pub async fn game_slug(&self, name: &str) -> Result<Option<String>> {
        let d = self.op("SlugRedirect", json!({ "name": name })).await?;
        Ok(d.get("game").and_then(|g| s(g, "slug")))
    }

    pub async fn current_drop(&self, channel_id: &str) -> Result<Option<CurrentDrop>> {
        let d = self
            .op("CurrentDrop", json!({ "channelID": channel_id }))
            .await?;
        let Some(sess) = d
            .pointer("/currentUser/dropCurrentSession")
            .filter(|v| v.is_object())
        else {
            return Ok(None);
        };
        let drop_id = s(sess, "dropID").unwrap_or_default();
        if drop_id.is_empty() {
            return Ok(None);
        }
        Ok(Some(CurrentDrop {
            drop_id,
            channel_id: sess.get("channel").and_then(|c| s(c, "id")),
            current_minutes: u(sess, "currentMinutesWatched").unwrap_or(0) as u32,
            required_minutes: u(sess, "requiredMinutesWatched").unwrap_or(0) as u32,
        }))
    }

    pub async fn claim_drop(&self, claim_id: &str) -> Result<ClaimResult> {
        let d = self
            .op("ClaimDrop", json!({"input": {"dropInstanceID": claim_id}}))
            .await?;
        let status = d
            .pointer("/claimDropRewards/status")
            .and_then(Value::as_str)
            .unwrap_or("");
        Ok(match status {
            "ELIGIBLE_FOR_ALL" => ClaimResult::Claimed,
            "DROP_INSTANCE_ALREADY_CLAIMED" => ClaimResult::AlreadyClaimed,
            "" => ClaimResult::Failed(d.to_string()),
            other => ClaimResult::Failed(other.to_string()),
        })
    }

    /// Returns `(token value, signature)` for the HLS playlist of a live channel.
    pub async fn playback_token(&self, login: &str) -> Result<(String, String)> {
        let d = self
            .op("PlaybackAccessToken", json!({ "login": login }))
            .await?;
        let t = d
            .get("streamPlaybackAccessToken")
            .filter(|t| t.is_object())
            .ok_or_else(|| Error::decode("no playback token (offline?)"))?;
        Ok((
            s(t, "value").ok_or_else(|| Error::decode("token value"))?,
            s(t, "signature").ok_or_else(|| Error::decode("token signature"))?,
        ))
    }

    /// Channels followed by the account.
    pub async fn follows(&self) -> Result<Vec<ChannelRef>> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..20 {
            let mut vars = json!({});
            if let Some(c) = &cursor {
                vars["cursor"] = json!(c);
            }
            let d = self.op("ChannelFollows", vars).await?;
            let follows = d.pointer("/user/follows").cloned().unwrap_or(Value::Null);
            let mut last = None;
            for e in arr(&follows, "edges") {
                last = s(e, "cursor");
                if let Some(n) = e.get("node") {
                    if let Some(id) = s(n, "id") {
                        out.push(ChannelRef {
                            id,
                            login: s(n, "login"),
                            display_name: s(n, "displayName"),
                        });
                    }
                }
            }
            let more = follows
                .pointer("/pageInfo/hasNextPage")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !more || last.is_none() || last == cursor {
                break;
            }
            cursor = last;
        }
        Ok(out)
    }

    pub async fn make_prediction(
        &self,
        event_id: &str,
        outcome_id: &str,
        points: u64,
    ) -> Result<Option<String>> {
        let tx = format!("{:032x}", rand::random::<u128>());
        let d = self
            .op(
                "MakePrediction",
                json!({"input": {"eventID": event_id, "outcomeID": outcome_id, "points": points, "transactionID": tx}}),
            )
            .await?;
        Ok(d.pointer("/makePrediction/error/code")
            .and_then(Value::as_str)
            .map(str::to_string))
    }

    pub async fn delete_notification(&self, id: &str) -> Result<()> {
        self.op("NotificationsDelete", json!({"input": {"id": id}}))
            .await
            .map(drop)
    }
}
