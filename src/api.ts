import { invoke } from '@tauri-apps/api/core'
import type { AppConfig, AuthInitResponse, DropCampaign, DropItem, MinerStatus, StreakMinerStatus } from './types'

export const api = {
  checkAuthStatus: () => invoke<boolean>('check_auth_status'),
  getUserInfo: () => invoke<any>('get_user_info'),
  startAuth: () => invoke<AuthInitResponse>('start_auth'),
  pollForToken: (deviceCode: string, interval: number) => invoke<string>('poll_for_token', { deviceCode, interval }),
  logout: () => invoke<void>('logout'),

  getStatus: () => invoke<MinerStatus>('get_status'),
  startMiner: () => invoke<void>('start_miner'),
  stopMiner: () => invoke<void>('stop_miner'),
  getDropCampaigns: () => invoke<DropCampaign[]>('get_drop_campaigns'),
  getClaimedDrops: () => invoke<DropItem[]>('get_claimed_drops'),

  getStreakMinerStatus: () => invoke<StreakMinerStatus>('get_streak_miner_status'),
  startStreakMiner: () => invoke<void>('start_streak_miner'),
  stopStreakMiner: () => invoke<void>('stop_streak_miner'),

  getConfig: () => invoke<AppConfig>('get_config'),
  saveConfig: (config: AppConfig) => invoke<void>('save_config', { config }),
  resetConfig: () => invoke<AppConfig>('reset_config'),
  searchGames: (query: string) => invoke<[string, string][]>('search_games_cmd', { query }),
}
