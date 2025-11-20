import { Upload } from 'lucide-react'
import { cn } from '@/lib/utils'

interface UploadAreaProps {
  isDragOver: boolean
  onSelect: () => void
}

export function UploadArea({ isDragOver, onSelect }: UploadAreaProps) {
  return (
    <div className="h-full px-8 pt-6">
      <div
        className={cn(
          'group max-w-2xl mx-auto relative h-full flex flex-col items-center justify-center p-12 rounded-2xl border-2 border-dashed transition-all duration-300 cursor-pointer',
          'hover:border-primary/70 hover:bg-primary/5',
          isDragOver ? 'border-primary bg-primary/10 scale-[1.01]' : 'border-border/50 bg-card/30'
        )}
        onClick={onSelect}
      >
        <div className="flex flex-col items-center justify-center h-full">
          <div className="p-5 rounded-2xl bg-primary inline-flex mb-4">
            <Upload className="w-8 h-8 text-primary-foreground" />
          </div>
          <h2 className="text-2xl font-medium text-foreground mb-2">Upload your videos</h2>
          <p className="text-muted-foreground mb-4">Click to select or drag and drop files</p>
          <div className="flex items-center justify-center gap-2 text-xs text-muted-foreground">
            <span>Supported:</span>
            {['MP4', 'MOV', 'MKV', 'AVI'].map((format) => (
              <span key={format} className="px-1.5 py-0.5 bg-muted rounded text-xs font-mono">
                {format}
              </span>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

