import { Palette } from 'lucide-react'
import { ColorPicker } from './ColorPicker'
import { Settings } from '../types'

interface ColorControlsProps {
  settings: Settings
  onUpdate: (updates: Partial<Settings>) => void
}

export function ColorControls({ settings, onUpdate }: ColorControlsProps) {
  return (
    <div className="p-4 border-b border-border/50">
      <h3 className="text-base font-medium text-foreground mb-3 flex items-center gap-2">
        <Palette className="w-4 h-4" />
        Color Controls
      </h3>
      <div className="space-y-3">
        <ColorPicker
          label="Text Color"
          value={settings.textColor}
          onChange={(value) => onUpdate({ textColor: value })}
        />
        <ColorPicker
          label="Highlight Color"
          value={settings.highlightWordColor}
          onChange={(value) => onUpdate({ highlightWordColor: value })}
        />
        <ColorPicker
          label="Outline Color"
          value={settings.outlineColor}
          onChange={(value) => onUpdate({ outlineColor: value })}
        />
      </div>
    </div>
  )
}

