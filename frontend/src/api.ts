import { invoke } from '@tauri-apps/api/core'
import type { AppConfig, AuthInitResponse, DropCampaign, DropItem, MinerStatus, StreakMinerStatus } from './types'

export interface UserInfo {
  id: string
  login: string
  displayName: string
  profileImageURL: string
}

interface GetUserInfoResponse {
  data?: {
    currentUser?: UserInfo
  }
}

export const api = {
  checkAuthStatus: () => invoke<boolean>('check_auth_status'),
  getUserInfo: () => invoke<GetUserInfoResponse>('get_user_info'),
  startAuth: () => invoke<AuthInitResponse>('start_auth'),
  pollForToken: (deviceCode: string, interval: number) => invoke<string>('poll_for_token', { deviceCode, interval }),
  logout: () => invoke<void>('logout'),

  getStatus: () => invoke<MinerStatus>('get_status'),
  startMiner: () => invoke<void>('start_miner'),
  stopMiner: () => invoke<void>('stop_miner'),
  getDropCampaigns: () => invoke<DropCampaign[]>('get_drop_campaigns'),
  getClaimedDrops: () => invoke<DropItem[]>('get_claimed_drops'),
  claimDrop: (dropId: string) => invoke<unknown>('claim_drop_cmd', { dropId }),

  getStreakMinerStatus: () => invoke<StreakMinerStatus>('get_streak_miner_status'),
  startStreakMiner: () => invoke<void>('start_streak_miner'),
  stopStreakMiner: () => invoke<void>('stop_streak_miner'),

  getConfig: () => invoke<AppConfig>('get_config'),
  saveConfig: (config: AppConfig) => invoke<void>('save_config', { config }),
  resetConfig: () => invoke<AppConfig>('reset_config'),
  searchGames: (query: string) => invoke<[string, string][]>('search_games_cmd', { query }),
  checkBatteryOptimizations: () => invoke<boolean>('check_battery_optimizations'),
  requestBatteryOptimization: () => invoke<void>('request_battery_optimization'),
  updateForegroundLabel: (label: string) => invoke<void>('update_foreground_label', { label }),

  /** Health check — resolves immediately if backend is reachable. */
  ping: () => invoke<void>('ping'),
}
