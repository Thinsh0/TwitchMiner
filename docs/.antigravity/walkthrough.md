# Twitch Miner Dashboard Fix — Session Log

## Session: 2026-04-26

### Task 1: Multi-drop display
- **Backend**: Added `ActiveDropInfo` struct, extended `MinerStatus` with `active_drops: Vec<ActiveDropInfo>`
- **Backend**: `summarize_active_drops` returns per-drop detail list
- **Frontend**: Added `ActiveDropList` component rendering each drop with image, name, individual progress bar
- **Frontend**: Updated store with `ActiveDropInfo` type

### Task 2: Per-campaign active drop selection  
- **Backend**: `collect_incomplete_time_based_drops_for_game` now picks only ONE drop per campaign (the one closest to completion)

### Task 3: Real-time progress via GQL
- **Backend**: Added `get_drop_current_session` (DropCurrentSessionContext) to api.rs
- **Backend**: After each Spade ping, call session API to update active_drops with accurate minute count

### Task 4: Fix Spade URL extraction (ROOT CAUSE)
- **Root cause**: Regex patterns searched for `beacon_url` + `video-edge-*.ts` — Twitch changed this
- **Fix**: Adopted TwitchDropsMiner's broader pattern `"spade_?url": ?"(https://[.\w\-/]+)"`
- **Fix**: Full desktop Chrome User-Agent for page fetch

### Task 5: Fix progress percentage calculation
- **Fix**: Removed `percentAwarded` fallback (always 0), always compute from `currentMinutesWatched / requiredMinutesWatched`

### Task 6: DropsView auto-refresh
- **Fix**: `useEffect` auto-fetches on mount + `setInterval` every 2min while miner is running

### Task 7: Parallel Drops Mining Mode
- **Feature**: Replaced single-stream target logic with parallel multi-target mining.
- **Backend**: Created `TargetStream` struct in `miner.rs` to hold stream state per game.
- **Backend**: `start_miner` now scans `inventory` to find up to 5 concurrent campaigns that have incomplete drops.
- **Backend**: Fetches `spade_url` for a streamer for each campaign, caching them in `active_streams`.
- **Backend**: Inner `miner_loop` sends `minute-watched` spade pings for ALL active streams in parallel every 60 seconds.
- **Backend**: Updates UI with aggregated multi-stream drop progress using `summarize_active_drops` over all target games.
