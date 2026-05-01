# Plan - Fix Drop Progress & Smart Switching (Refined)

## Tasks
- [ ] Update `TargetStream` struct with tracking fields (`last_progress_min`, `last_progress_at`, `failed_streamers`)
- [ ] Refactor `start_miner` loop to use mutable `active_streams`
- [ ] Implement event-based sync (emit only on change)
- [ ] Implement Smart Switch (6-min stagnation threshold, 2 streamers limit)
- [ ] Implement 1-hour cooldown for stagnant drop campaigns
- [ ] Implement immediate loop break on 100% completion
- [ ] Verify fix and auto-switching functionality

## Status
Analyzing issue... Refined plan with 6-min threshold and 1-hour cooldown created. Serialization safety and state management issues addressed.
