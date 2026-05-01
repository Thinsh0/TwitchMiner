import { create } from 'zustand'
import { activityNotifications, activityStorage } from './activity'
import type { ActivityEvent, ActivityEventType, DropCampaign, MinerStatus, DropItem } from './types'
export type { ActiveDropInfo, DropCampaign, DropItem, MinerStatus } from './types'

interface AppState {
  status: MinerStatus
  setStatus: (status: MinerStatus) => void
  isAuthenticated: boolean
  setAuthenticated: (val: boolean) => void
  dropCampaigns: DropCampaign[]
  setDropCampaigns: (campaigns: DropCampaign[]) => void
  activities: ActivityEvent[]
  addActivity: (event: Omit<ActivityEvent, 'id' | 'timestamp'>) => void
  setActivities: (events: ActivityEvent[]) => void
  loadActivities: () => Promise<void>
  claimedDrops: DropItem[]
  setClaimedDrops: (drops: DropItem[]) => void
  addClaimedDrop: (drop: DropItem) => void
  loadClaimedDrops: () => Promise<void>
}

const activityTitles: Record<ActivityEventType, string> = {
  drop: 'Drops',
  points: 'Channel Points',
  auth: 'Authentication',
  system: 'System',
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
  activities: [],
  addActivity: (event) => set((state) => ({
    activities: (() => {
      const nextEvent = {
        ...event,
        title: event.title || activityTitles[event.type],
        id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
        timestamp: Date.now(),
      }
      const nextActivities = [nextEvent, ...state.activities].slice(0, 25)
      void activityStorage.save(nextActivities)
      void activityNotifications.notify(nextEvent)
      return nextActivities
    })(),
  })),
  setActivities: (events) => set({ activities: events.slice(0, 25) }),
  loadActivities: async () => {
    const events = await activityStorage.load()
    set({ activities: events })
  },
  claimedDrops: [],
  setClaimedDrops: (drops) => set({ claimedDrops: drops }),
  addClaimedDrop: (drop) => set((state) => ({ claimedDrops: [drop, ...state.claimedDrops] })),
  loadClaimedDrops: async () => {
    const { api } = await import('./api')
    try {
      const drops = await api.getClaimedDrops()
      set({ claimedDrops: drops.reverse() }) // Reverse to show newest first
    } catch (e) {
      console.error('Failed to load claimed drops', e)
    }
  },
}))