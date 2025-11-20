import { Button } from '@/app/components/ui/button'
import { Trash, FileVideo } from 'lucide-react'

interface FileCardProps {
  path: string
  onRemove: () => void
  progress?: number
  status?: string
}

export function FileCard({ path, onRemove, progress, status }: FileCardProps) {
  const fileName = path.split('/').pop() || ''
  const showProgress = progress !== undefined

  return (
    <div className="group relative bg-gradient-to-br from-card/80 to-card/40 border border-border/30 rounded-xl p-4 hover:border-primary/40 transition-all duration-300 backdrop-blur-sm">
      <div className="flex items-center gap-4">
        <div className="relative">
          <FileVideo className="w-6 h-6 text-white" />
        </div>

        <div className="flex-1 min-w-0">
          <p className="font-medium text-foreground truncate text-sm">{fileName}</p>
          {showProgress && (
            <div className="mt-2 space-y-1">
              <div className="flex items-center justify-between text-xs">
                <span className="text-muted-foreground">{status || 'Processing...'}</span>
                <span className="text-muted-foreground font-medium">{progress}%</span>
              </div>
              <div className="w-full bg-muted/30 rounded-full h-1.5 overflow-hidden">
                <div
                  className="h-full bg-primary transition-all duration-300 ease-out rounded-full"
                  style={{ width: `${progress}%` }}
                />
              </div>
            </div>
          )}
        </div>

        <Button
          variant="ghost"
          size="sm"
          className="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive hover:!bg-destructive/10 transition-all duration-200 rounded-lg"
          onClick={onRemove}
          disabled={showProgress}
        >
          <Trash className="w-4 h-4" />
        </Button>
      </div>
    </div>
  )
}

