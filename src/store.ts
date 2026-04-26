import { create } from 'zustand'

export interface ActiveDropInfo {
  id: string
  name: string
  image_url: string | null
  current_minutes: number
  required_minutes: number
  /** Individual progress percentage (0-100). */
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
}

export interface DropCampaign {
  id: string;
  name: string;
  game_name: string;
  image_url?: string;
  status: string;
  start_date: string;
  end_date: string;
  drops: DropItem[];
}

export interface DropItem {
  id: string;
  name: string;
  required_minutes: number;
  current_minutes: number;
  is_claimed: boolean;
  image_url?: string;
}

interface AppState {
  status: MinerStatus
  setStatus: (status: MinerStatus) => void
  isAuthenticated: boolean
  setAuthenticated: (val: boolean) => void
  dropCampaigns: DropCampaign[]
  setDropCampaigns: (campaigns: DropCampaign[]) => void
}

export const useAppStore = create<AppState>((set) => ({
  status: {
    status: 'Idle',
    is_running: false,
    current_streamer: null,
    current_streamer_login: null,
    current_drop: null,
    drop_image_url: null,
    streamer_image_url: null,
    progress: 0,
    active_drops: [],
    current_points: '0',
  },
  setStatus: (status) => set({ status }),
  isAuthenticated: false,
  setAuthenticated: (val) => set({ isAuthenticated: val }),
  dropCampaigns: [],
  setDropCampaigns: (campaigns) => set({ dropCampaigns: campaigns }),
}))