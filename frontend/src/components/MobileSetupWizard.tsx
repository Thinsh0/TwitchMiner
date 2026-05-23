import { useState, useEffect, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification'
import { platform } from '@tauri-apps/plugin-os'
import { api } from '@/api'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { 
  CheckCircle2, XCircle, ShieldAlert, 
  ExternalLink, Smartphone, AlertTriangle, Info 
} from 'lucide-react'

type Brand = 'xiaomi' | 'samsung' | 'huawei' | 'oneplus' | 'other';

export function MobileSetupWizard() {
  const [isAndroid, setIsAndroid] = useState(false)
  const [notificationsGranted, setNotificationsGranted] = useState<boolean | null>(null)
  const [batteryOptimizationsIgnored, setBatteryOptimizationsIgnored] = useState<boolean | null>(null)
  const [selectedBrand, setSelectedBrand] = useState<Brand>('other')
  const [isOpen, setIsOpen] = useState(false)

  const checkPermissions = useCallback(async () => {
    try {
      const notif = await isPermissionGranted()
      setNotificationsGranted(notif)
      
      const battery = await api.checkBatteryOptimizations()
      setBatteryOptimizationsIgnored(battery)
    } catch (err) {
      console.error('Failed to check mobile permissions:', err)
    }
  }, [])

  useEffect(() => {
    // Check if we are on Android
    try {
      const p = platform()
      const android = p === 'android'
      setIsAndroid(android)
      if (android) {
        checkPermissions()
        // Auto-open if setup is incomplete
        setTimeout(async () => {
          const notif = await isPermissionGranted()
          const battery = await api.checkBatteryOptimizations()
          if (!notif || !battery) {
            setIsOpen(true)
          }
        }, 1000)
      }
    } catch (e) {
      console.error('Platform check failed:', e)
    }
  }, [checkPermissions])

  // Poll permissions when window gains focus or when dialog is open
  useEffect(() => {
    if (!isAndroid || !isOpen) return

    const handleFocus = () => {
      checkPermissions()
    }

    window.addEventListener('focus', handleFocus)
    const interval = setInterval(checkPermissions, 3000)

    return () => {
      window.removeEventListener('focus', handleFocus)
      clearInterval(interval)
    }
  }, [isAndroid, isOpen, checkPermissions])

  const grantNotifications = async () => {
    try {
      const permission = await requestPermission()
      setNotificationsGranted(permission === 'granted')
    } catch (err) {
      console.error('Request notification permission failed:', err)
    }
  }

  const disableBatteryOptimization = async () => {
    try {
      await api.requestBatteryOptimization()
    } catch (err) {
      console.error('Request battery exemption failed:', err)
    }
  }

  if (!isAndroid) return null

  const isSetupComplete = notificationsGranted && batteryOptimizationsIgnored

  const brandGuides: Record<Brand, { name: string; steps: string[] }> = {
    xiaomi: {
      name: 'Xiaomi / Poco / Redmi',
      steps: [
        'Open System Settings -> Apps -> Manage Apps -> Twitch Miner.',
        'Enable the "Autostart" toggle (extremely important!).',
        'Tap on "Battery Saver" and select "No restrictions" (Pas de restrictions).',
        'Enable all Notification options if any are off.'
      ]
    },
    samsung: {
      name: 'Samsung',
      steps: [
        'Open Settings -> Apps -> Twitch Miner.',
        'Tap on "Battery" (or App battery usage).',
        'Change setting from "Optimized" or "Restricted" to "Unrestricted" (Non restreint).',
        'Ensure "Remove permissions if app is unused" is disabled.'
      ]
    },
    huawei: {
      name: 'Huawei / Honor',
      steps: [
        'Open Settings -> Battery -> App launch.',
        'Find Twitch Miner and turn off "Manage automatically".',
        'In the popup, enable "Auto-launch", "Secondary launch", and "Run in background".'
      ]
    },
    oneplus: {
      name: 'OnePlus / Oppo / Realme',
      steps: [
        'Open Settings -> Apps -> App management -> Twitch Miner.',
        'Tap on "Battery usage".',
        'Enable "Allow background activity" and "Allow auto-launch".'
      ]
    },
    other: {
      name: 'Google Pixel / Motorola / Stock Android',
      steps: [
        'Open Settings -> Apps -> All Apps -> Twitch Miner.',
        'Tap on "App Battery Usage" (or Battery).',
        'Select "Unrestricted" to allow background activity without limitations.'
      ]
    }
  }

  return (
    <>
      {/* Alert banner on Dashboard if setup is incomplete */}
      {!isSetupComplete && (
        <div className="mb-6 p-4 rounded-xl bg-amber-500/10 border border-amber-500/20 text-amber-300 flex items-center justify-between gap-4 backdrop-blur-md">
          <div className="flex items-center gap-3">
            <ShieldAlert className="w-6 h-6 text-amber-400 flex-shrink-0" />
            <div>
              <p className="font-semibold text-sm">Background Mining Incomplete</p>
              <p className="text-xs text-amber-400/80">Device settings may kill the miner in background. Let\'s fix it.</p>
            </div>
          </div>
          <Button 
            size="sm" 
            onClick={() => setIsOpen(true)}
            className="bg-amber-600 hover:bg-amber-700 text-white border-none shadow-md shrink-0"
          >
            Configure Setup
          </Button>
        </div>
      )}



      {/* Main Setup Dialog */}
      {isOpen && createPortal(
        <div className="fixed inset-0 bg-black/80 backdrop-blur-lg flex items-start justify-center p-4 sm:p-6 z-[100] overflow-y-auto" style={{ paddingTop: 'env(safe-area-inset-top, 2rem)', paddingBottom: 'env(safe-area-inset-bottom, 2rem)' }}>
          <Card className="w-full max-w-xl bg-zinc-950 border-white/10 text-white shadow-2xl my-auto flex flex-col max-h-[90vh]">
            <CardHeader className="border-b border-white/5 pb-4 shrink-0">
              <div className="flex justify-between items-start">
                <div>
                  <CardTitle className="text-2xl font-bold flex items-center gap-2">
                    <Smartphone className="w-6 h-6 text-purple-400" />
                    Mobile Setup Helper
                  </CardTitle>
                  <CardDescription className="text-zinc-400 mt-1">
                    Configure your phone so the background miner is never killed.
                  </CardDescription>
                </div>
                <Button 
                  variant="ghost" 
                  size="sm" 
                  onClick={() => setIsOpen(false)}
                  className="text-zinc-400 hover:text-white"
                >
                  ✕
                </Button>
              </div>
            </CardHeader>
            <CardContent className="p-6 space-y-6 overflow-y-auto flex-1">
              
              {/* Notifications Card */}
              <div className="p-4 rounded-xl bg-white/5 border border-white/5 space-y-3">
                <div className="flex items-center justify-between">
                  <h3 className="font-semibold text-sm flex items-center gap-2">
                    1. Push Notifications
                  </h3>
                  {notificationsGranted ? (
                    <Badge className="bg-green-500/20 text-green-400 border border-green-500/30 flex items-center gap-1">
                      <CheckCircle2 className="w-3.5 h-3.5" /> Granted
                    </Badge>
                  ) : (
                    <Badge className="bg-red-500/20 text-red-400 border border-red-500/30 flex items-center gap-1">
                      <XCircle className="w-3.5 h-3.5" /> Restricted
                    </Badge>
                  )}
                </div>
                <p className="text-xs text-zinc-400 leading-relaxed">
                  Android requires a persistent notification to keep background jobs running. Without this, the OS will instantly pause the mining loop when you close the app.
                </p>
                {!notificationsGranted && (
                  <Button 
                    size="sm" 
                    onClick={grantNotifications}
                    className="w-full bg-purple-600 hover:bg-purple-700 text-white text-xs py-1.5"
                  >
                    Grant Notification Permission
                  </Button>
                )}
              </div>

              {/* Battery Optimizations Card */}
              <div className="p-4 rounded-xl bg-white/5 border border-white/5 space-y-3">
                <div className="flex items-center justify-between">
                  <h3 className="font-semibold text-sm flex items-center gap-2">
                    2. Battery Saver Bypass
                  </h3>
                  {batteryOptimizationsIgnored ? (
                    <Badge className="bg-green-500/20 text-green-400 border border-green-500/30 flex items-center gap-1">
                      <CheckCircle2 className="w-3.5 h-3.5" /> Optimized
                    </Badge>
                  ) : (
                    <Badge className="bg-red-500/20 text-red-400 border border-red-500/30 flex items-center gap-1">
                      <XCircle className="w-3.5 h-3.5" /> Restricting
                    </Badge>
                  )}
                </div>
                <p className="text-xs text-zinc-400 leading-relaxed">
                  By default, Android puts background apps to sleep to save battery. Disable battery optimization for Twitch Miner so it can farm points and drops overnight.
                </p>
                {!batteryOptimizationsIgnored && (
                  <Button 
                    size="sm" 
                    onClick={disableBatteryOptimization}
                    className="w-full bg-purple-600 hover:bg-purple-700 text-white text-xs py-1.5"
                  >
                    Open System Battery Settings
                  </Button>
                )}
              </div>

              {/* Auto-Start & Brand Instructions */}
              <div className="p-4 rounded-xl bg-white/5 border border-white/5 space-y-4">
                <div className="flex items-center justify-between">
                  <h3 className="font-semibold text-sm flex items-center gap-2">
                    3. Brand-Specific Autostart
                  </h3>
                  <Badge className="bg-blue-500/20 text-blue-400 border border-blue-500/30 flex items-center gap-1">
                    <Info className="w-3.5 h-3.5" /> Manual Action
                  </Badge>
                </div>
                <p className="text-xs text-zinc-400 leading-relaxed">
                  Manufacturers (Xiaomi, Samsung, Huawei, etc.) have extremely aggressive task killers. You must manually allow Twitch Miner to launch automatically and run in the background.
                </p>

                {/* Brand Selector */}
                <div className="space-y-1.5">
                  <label className="text-xs text-zinc-500 block font-medium">Select your phone brand:</label>
                  <div className="grid grid-cols-3 gap-2">
                    {(Object.keys(brandGuides) as Brand[]).map((brand) => (
                      <button
                        key={brand}
                        onClick={() => setSelectedBrand(brand)}
                        className={`px-2 py-1.5 rounded-lg border text-xs font-medium transition-all text-center truncate ${
                          selectedBrand === brand
                            ? 'bg-purple-600/20 border-purple-500 text-purple-400'
                            : 'border-white/5 bg-white/5 text-zinc-400 hover:text-white'
                        }`}
                      >
                        {brand === 'other' ? 'Pixel/Other' : brand.toUpperCase()}
                      </button>
                    ))}
                  </div>
                </div>

                {/* Steps box */}
                <div className="p-3 rounded-lg bg-black/40 border border-white/5 space-y-2">
                  <span className="text-xs font-semibold text-purple-300 block">
                    Steps for {brandGuides[selectedBrand].name}:
                  </span>
                  <ol className="list-decimal pl-4 text-xs text-zinc-300 space-y-1.5">
                    {brandGuides[selectedBrand].steps.map((step, idx) => (
                      <li key={idx} className="leading-normal">{step}</li>
                    ))}
                  </ol>
                </div>

                {/* External link */}
                <div className="flex items-center gap-1.5 text-xs text-zinc-400 pt-1">
                  <AlertTriangle className="w-4 h-4 text-yellow-500" />
                  <span>Not working? Read details on</span>
                  <a 
                    href="https://dontkillmyapp.com" 
                    target="_blank" 
                    rel="noreferrer" 
                    className="text-purple-400 hover:underline flex items-center gap-0.5"
                  >
                    Don\'t Kill My App <ExternalLink className="w-3 h-3" />
                  </a>
                </div>
              </div>

              {/* Status footer button */}
              <div className="pt-4 border-t border-white/5 flex justify-end">
                <Button 
                  className={isSetupComplete ? "bg-green-600 hover:bg-green-700 text-white" : "bg-purple-600 hover:bg-purple-700 text-white"}
                  onClick={() => setIsOpen(false)}
                >
                  {isSetupComplete ? "All Configured! Close" : "Close Wizard"}
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>,
        document.body
      )}
    </>
  )
}
