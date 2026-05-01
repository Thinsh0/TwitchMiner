# Context - Drop Progress & Smart Switching

## Decision: Update `active_streams` in-place
We found that the mining loop was using a stale copy of the `active_streams` metadata, which caused the dashboard to reset to old values every minute. We decided to make `active_streams` mutable and update its internal `active_drop` info whenever new data arrives.

## Decision: Smart Stagnation Detection (Smart Switch)
Following the user's idea, we decided to implement a detection mechanism. 
- **Threshold**: 6 minutes (to account for Twitch batching).
- **Strategy**: If no progress, try another streamer.
- **Cooldown**: If two streamers fail, skip the drop for 1 hour.

## Decision: Serialization Safety
Even if internal, tracking fields like `Instant` or `failed_streamers` will be handled carefully to avoid Serde/Tauri serialization issues.

## Decision: Event-driven Updates
To optimize performance, the backend will only emit updates when a meaningful change occurs (progress increase, switch, or completion).
