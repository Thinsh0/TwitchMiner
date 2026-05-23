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
    <div className="flex h-screen items-center justify-center bg-black/95 relative overflow-hidden">
      <Toaster richColors position="top-center" />
      <div className="absolute top-[-20%] left-[-10%] w-[500px] h-[500px] bg-purple-600/30 blur-[120px] rounded-full" />
      <div className="absolute bottom-[-20%] right-[-10%] w-[400px] h-[400px] bg-blue-600/20 blur-[100px] rounded-full" />

      <Card className="w-[400px] bg-black/40 border-white/10 backdrop-blur-xl z-10 shadow-2xl">
        <CardHeader>
          <CardTitle className="text-2xl font-bold text-white">Twitch Miner V2</CardTitle>
          <CardDescription className="text-zinc-400">Please authenticate to start mining.</CardDescription>
        </CardHeader>
        <CardContent>
          <Button className="w-full bg-purple-600 hover:bg-purple-700 text-white" onClick={startAuth} disabled={authLoading}>
            {authLoading ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            Login with Twitch
          </Button>
        </CardContent>
      </Card>

      <Dialog open={authDialog} onOpenChange={setAuthDialog}>
        <DialogContent className="bg-zinc-950 border-white/10 text-white sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Link your Account</DialogTitle>
            <DialogDescription className="text-zinc-400">
              Go to <a href="#" onClick={(e) => { e.preventDefault(); openUrl(authUri) }} className="text-purple-400 hover:underline">{authUri}</a> and enter the code below:
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col items-center justify-center space-y-2 py-6 cursor-pointer group" onClick={handleCopyCode}>
            <span className="text-4xl font-mono tracking-[0.3em] font-bold text-purple-400 group-hover:text-purple-300 transition-colors">
              {userCode}
            </span>
            <span className={`text-sm transition-opacity ${codeCopied ? 'text-green-400 opacity-100' : 'text-zinc-500 opacity-0 group-hover:opacity-100'}`}>
              {codeCopied ? <><Check className="w-4 h-4 inline mr-1" />Copied!</> : <><Copy className="w-4 h-4 inline mr-1" />Click to copy</>}
            </span>
          </div>
          <DialogFooter className="sm:justify-start">
            <p className="text-sm text-zinc-500 flex items-center">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" /> Waiting for confirmation...
            </p>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
