export interface ActiveDropInfo {
  id: string
  name: string
  image_url: string | null
  current_minutes: number
  required_minutes: number
  progress: number
}

export interface MinerStatus {
  status: string
  is_running: boolean
  current_streamer: string | null
  current_streamer_login: string | null
  current_drop: string | null
  drop_image_url: string | null
  streamer_image_url: string | null
  progress: number
  current_points: string
  active_drops: ActiveDropInfo[]
  started_at?: string
}

export interface DropItem {
  id: string
  name: string
  required_minutes: number
  current_minutes: number
  is_claimed: boolean
  image_url: string | null
}

export interface DropCampaign {
  id: string
  name: string
  game_name: string
  image_url: string | null
  status: string
  start_date: string
  end_date: string
  drops: DropItem[]
}

export interface ChannelPointsInfo {
  channel_name: string
  points: number
  has_claim_available: boolean
  status: string
}

export interface StreakMinerStatus {
  status: string
  current_channels: string[]
  total_pings_sent: number
  successful_pings: number
  failed_pings: number
  cycle_count: number
  channels_info: ChannelPointsInfo[]
  started_at?: string
}

export interface AuthInitResponse {
  device_code: string
  user_code: string
  verification_uri: string
  expires_in: number
  interval: number
}

export interface AppConfig {
  client_id: string
  priorities: {
    games: string[]
    streamers: string[]
  }
  bonus: {
    daily_streaks: {
      channels: string[]
      auto_start: boolean
      mode: string
    }
    points_farming: {
      channels: string[]
      auto_start: boolean
      max_concurrent: number
    }
  }
  settings: {
    language: string
    check_interval: number
    notifications_enabled: boolean
    notify_active_drop: boolean
    notify_claim: boolean
    notify_campaign_finish: boolean
    minimize_to_tray?: boolean
    show_tray_icon?: boolean
    auto_start_drops: boolean
    mine_all_inventory?: boolean
  }
}

export type ActivityEventType = 'drop' | 'points' | 'auth' | 'system'

export interface ActivityEvent {
  id: string
  type: ActivityEventType
  title: string
  description: string
  timestamp: number
}
