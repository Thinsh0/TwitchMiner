import { Store } from '@tauri-apps/plugin-store'
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification'
import type { ActivityEvent, AppConfig } from './types'

const ACTIVITY_STORE = 'activity.json'
const ACTIVITY_KEY = 'recent_activity'
const MAX_ACTIVITIES = 25

let storePromise: Promise<Store> | null = null
let configPromise: Promise<AppConfig> | null = null

const getStore = () => {
  if (!storePromise) {
    storePromise = Store.load(ACTIVITY_STORE)
  }
  return storePromise
}

export const activityStorage = {
  async load(): Promise<ActivityEvent[]> {
    try {
      const store = await getStore()
      const events = await store.get<ActivityEvent[]>(ACTIVITY_KEY)
      return Array.isArray(events) ? events.slice(0, MAX_ACTIVITIES) : []
    } catch (err) {
      console.error('Failed to load activity history:', err)
      return []
    }
  },

  async save(events: ActivityEvent[]): Promise<void> {
    try {
      const store = await getStore()
      await store.set(ACTIVITY_KEY, events.slice(0, MAX_ACTIVITIES))
      await store.save()
    } catch (err) {
      console.error('Failed to save activity history:', err)
    }
  },
}

const getNotificationPermission = async () => {
  let permissionGranted = await isPermissionGranted()
  if (!permissionGranted) {
    const permission = await requestPermission()
    permissionGranted = permission === 'granted'
  }
  return permissionGranted ? 'granted' : 'denied'
}

const shouldNotify = (event: ActivityEvent, config: AppConfig) => {
  if (event.silent) return false
  if (!config.settings.notifications_enabled) return false
  if (event.type === 'drop') return config.settings.notify_active_drop
  if (event.type === 'points') return config.settings.notify_claim
  return event.type === 'auth' || event.type === 'system'
}

export const activityNotifications = {
  setConfig(config: AppConfig): void {
    configPromise = Promise.resolve(config)
  },

  async notify(event: ActivityEvent): Promise<void> {
    try {
      const config = await configPromise
      if (!config || !shouldNotify(event, config)) return
      const permission = await getNotificationPermission()
      if (permission !== 'granted') return
      sendNotification({ 
        title: event.title, 
        body: event.description,
        icon: event.image_url || undefined,
        attachments: event.image_url ? [{ id: 'drop_image', url: event.image_url }] : undefined 
      })
    } catch (err) {
      console.error('Failed to send activity notification:', err)
    }
  },
}
