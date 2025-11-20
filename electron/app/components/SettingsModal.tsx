import { useEffect, useState } from 'react'
import { Button } from '@/app/components/ui/button'
import { Key } from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogFooter,
  DialogClose,
} from '@/app/components/ui/dialog'
import { Input } from './ui/input'

interface SettingsModalProps {
  onSave: (apiKey: string) => void
  isOpen: boolean
  onOpenChange: (open: boolean) => void
  apiKey: string
}

export function SettingsModal({ onSave, isOpen, onOpenChange, apiKey }: SettingsModalProps) {
  const [apiKeyState, setApiKeyState] = useState(apiKey)

  useEffect(() => {
    setApiKeyState(apiKey)
  }, [apiKey])

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="w-9 h-9 text-muted-foreground hover:text-foreground bg-card/80 backdrop-blur-sm border border-border/50 hover:bg-card/90 transition-all duration-200 focus:ring-0 focus-visible:ring-0 ring-0"
        >
          <Key className="!w-3.5 !h-3.5" />
        </Button>
      </DialogTrigger>

      <DialogContent className="max-w-[450px]">
        <DialogHeader>
          <DialogTitle className="text-xl font-medium text-white/90">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-lg bg-primary/20">
                <Key className="w-5 h-5 text-primary" />
              </div>
              <h2 className="text-xl font-medium text-foreground">API Settings</h2>
            </div>
          </DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-4 py-4">
          <div className="grid gap-2">
            <label htmlFor="name" className="text-sm text-white/40 font-medium">
              OpenAI Api Key
            </label>
            <Input
              id="key"
              placeholder="sk-..."
              className="w-full px-4 py-3 border border-border rounded-lg bg-background/50 focus:outline-none hover:border-primary/50 focus:border-primary/70 ring-0 focus-visible:ring-2 focus-visible:ring-primary/20 focus:ring-2 focus:ring-primary/20 text-foreground transition-all duration-200"
              value={apiKeyState}
              onChange={(e) => setApiKeyState(e.target.value)}
            />
          </div>
        </div>

        <DialogFooter className="gap-4">
          <DialogClose asChild>
            <Button variant="outline" className="flex-1 bg-transparent text-foreground">
              Cancel
            </Button>
          </DialogClose>
          <DialogClose asChild>
            <Button
              onClick={() => {
                onSave(apiKeyState)
                onOpenChange(false)
              }}
              className="flex-1 bg-primary text-primary-foreground hover:bg-primary/90"
            >
              Save Settings
            </Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

