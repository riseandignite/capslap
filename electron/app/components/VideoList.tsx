import { Button } from '@/app/components/ui/button'
import { cn } from '@/lib/utils'
import { FileCard } from './FileCard'

type ProgressData = {
  progress: number
  status: string
}

interface VideoListProps {
  videos: string[]
  progressMap?: Map<string, ProgressData>
  onRemove: (path: string) => void
  onClearAll: () => void
}

export function VideoList({ videos, progressMap, onRemove, onClearAll }: VideoListProps) {
  return (
    <div className="flex justify-center w-full px-8 mx-auto">
      <div className="space-y-4 w-full max-w-2xl">
        <div className="flex items-center justify-between sticky top-0 bg-background/80 backdrop-blur-xs z-10 rounded-lg py-2 pt-6">
          <h3 className="text-lg font-medium text-foreground flex items-center gap-2">Uploaded files</h3>
          <div className="flex items-center gap-2">
            {videos.length > 1 && (
              <Button
                variant="ghost"
                size="sm"
                onClick={onClearAll}
                className="text-muted-foreground hover:text-foreground text-xs"
              >
                Clear all
              </Button>
            )}
          </div>
        </div>

        <div
          className={cn(
            'grid gap-3',
            videos.length <= 4 ? 'grid-cols-1' : 'grid-cols-1 lg:grid-cols-2'
          )}
        >
          {videos.map((path, index) => {
            const progressData = progressMap?.get(path)
            return (
              <FileCard
                key={index}
                path={path}
                onRemove={() => onRemove(path)}
                progress={progressData?.progress}
                status={progressData?.status}
              />
            )
          })}
        </div>
      </div>
    </div>
  )
}

