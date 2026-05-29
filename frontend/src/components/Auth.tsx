import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Loader2, Copy, Check } from 'lucide-react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { Toaster } from 'sonner'
import { useState } from 'react'

interface AuthScreenProps {
  authLoading: boolean;
  authDialog: boolean;
  authUri: string;
  userCode: string;
  startAuth: () => void;
  setAuthDialog: (open: boolean) => void;
}

export function AuthScreen({
  authLoading,
  authDialog,
  authUri,
  userCode,
  startAuth,
  setAuthDialog,
}: AuthScreenProps) {
  const [codeCopied, setCodeCopied] = useState(false);

  const handleCopyCode = async () => {
    try {
      const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
      await writeText(userCode)
    } catch {
      await navigator.clipboard.writeText(userCode)
    }
    setCodeCopied(true)
    setTimeout(() => setCodeCopied(false), 2000)
  }

  return (
    <div className="h-screen w-full flex flex-col items-center justify-center bg-background text-foreground relative overflow-hidden font-sans">
      <Toaster richColors position="top-center" />
      <div className="absolute top-0 left-0 w-full h-full bg-gradient-to-br from-purple-900/10 via-transparent to-blue-900/10 pointer-events-none" />
      <div className="absolute bottom-[-20%] right-[-10%] w-[400px] h-[400px] bg-blue-600/20 blur-[100px] rounded-full" />

      <Card className="w-[400px] bg-card border-border backdrop-blur-xl z-10 shadow-2xl">
        <CardHeader>
          <CardTitle className="text-2xl font-bold">Twitch Miner V2</CardTitle>
          <CardDescription>Please authenticate to start mining.</CardDescription>
        </CardHeader>
        <CardContent>
          <Button 
            className="w-full bg-purple-600 hover:bg-purple-700 text-white font-medium py-6 text-lg rounded-xl shadow-[0_0_20px_rgba(147,51,234,0.3)] hover:shadow-[0_0_30px_rgba(147,51,234,0.5)] transition-all duration-300" 
            onClick={startAuth} 
            disabled={authLoading}
          >
            {authLoading ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            {authLoading ? 'Connecting...' : 'Connect with Twitch'}
          </Button>
        </CardContent>
      </Card>

      <Dialog open={authDialog} onOpenChange={setAuthDialog}>
        <DialogContent className="bg-background border-border text-foreground sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Twitch Authentication</DialogTitle>
            <DialogDescription className="text-muted-foreground">
              Go to <a href="#" onClick={(e) => { e.preventDefault(); openUrl(authUri) }} className="text-purple-500 hover:underline">{authUri}</a> and enter the code below:
            </DialogDescription>
          </DialogHeader>
          <div className="bg-muted/50 p-4 rounded-lg flex flex-col items-center justify-center space-y-4 cursor-pointer group border border-border" onClick={handleCopyCode}>
            <span className="text-4xl font-mono tracking-[0.3em] font-bold text-purple-500 dark:text-purple-400">
              {userCode}
            </span>
            <span className={`text-sm transition-opacity ${codeCopied ? 'text-green-500 opacity-100' : 'text-muted-foreground opacity-0 group-hover:opacity-100'}`}>
              {codeCopied ? <><Check className="w-4 h-4 inline mr-1" />Copied!</> : <><Copy className="w-4 h-4 inline mr-1" />Click to copy</>}
            </span>
          </div>
          <DialogFooter className="sm:justify-start">
            <p className="text-sm text-muted-foreground mt-2 text-center w-full">
              Connect your Twitch account securely to start mining channel points and drops in the background.
            </p>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
