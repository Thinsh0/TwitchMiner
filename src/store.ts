import { create } from 'zustand'

export interface MinerStatus {
  status: string
  is_running: boolean
  current_streamer: string | null
  current_drop: string | null
  drop_image_url: string | null
  streamer_image_url: string | null
  progress: number
  current_points: string
}

interface AppState {
  status: MinerStatus
  setStatus: (status: MinerStatus) => void
  isAuthenticated: boolean
  setAuthenticated: (val: boolean) => void
}

export const useAppStore = create<AppState>((set) => ({
  status: {
    status: 'Idle',
    is_running: false,
    current_streamer: null,
    current_drop: null,
    drop_image_url: null,
    streamer_image_url: null,
    progress: 0,
    current_points: '0',
  },
  setStatus: (status) => set({ status }),
  isAuthenticated: false,
  setAuthenticated: (val) => set({ isAuthenticated: val }),
}))