# Tasks - Startup Notifications, Status Shortening, Claiming Fix, and Work Schedule

- `[x]` Fix startup notifications in `frontend/src/App.tsx`
- `[x]` Shorten warning status strings in `core/src/miner.rs` and `core/src/streak_miner.rs`
- `[x]` Claiming Fixes:
  - `[x]` Update query hash for `DropsPage_ClaimDropRewards` in `core/src/api.rs`
  - `[x]` Add `claim_drop_cmd` in `core/src/miner.rs`
  - `[x]` Register `claim_drop_cmd` in `desktop/src-tauri/src/lib.rs` and `mobile/src-tauri/src/lib.rs`
  - `[x]` Bind `claimDrop` in `frontend/src/api.ts`
  - `[x]` Bind "Claim" button in `frontend/src/components/DropsView.tsx` with loading state
- `[x]` Work Schedule System:
  - `[x]` Define `WorkSchedule`, `ScheduleDay`, and `ScheduleException` in `core/src/models/mod.rs`
  - `[x]` Add schedule checks to loops in `core/src/miner.rs` and `core/src/streak_miner.rs`
  - `[x]` Update TS interfaces in `frontend/src/types.ts`
  - `[x]` Implement "Schedule" tab in `frontend/src/components/Settings.tsx`
- `[x]` Verification:
  - `[x]` Build check for all projects (Core, Desktop, Mobile, Frontend)
  - `[x]` Test schedule logic and manual claiming
