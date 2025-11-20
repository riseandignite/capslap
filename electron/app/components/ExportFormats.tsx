import { Download } from 'lucide-react'
import { cn } from '@/lib/utils'
import { Settings, availableExportFormats } from '../types'

interface ExportFormatsProps {
  settings: Settings
  onToggleFormat: (formatId: string) => void
}

export function ExportFormats({ settings, onToggleFormat }: ExportFormatsProps) {
  return (
    <div className="p-4">
      <div>
        <h3 className="text-base font-medium text-foreground mb-3 flex items-center gap-2">
          <Download className="w-4 h-4" />
          Export Formats
        </h3>
        <div className="grid grid-cols-2 gap-2">
          {availableExportFormats.map((format) => (
            <div
              key={format.id}
              className={cn(
                'p-2 rounded-sm border text-center cursor-pointer transition-all duration-200 text-xs',
                settings.exportFormats?.includes(format.id)
                  ? 'border-primary/70 text-primary'
                  : 'border-border/50 text-muted-foreground hover:border-primary/50'
              )}
              onClick={() => onToggleFormat(format.id)}
            >
              {format.name}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

