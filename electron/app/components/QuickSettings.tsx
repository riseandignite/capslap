import { Settings } from 'lucide-react'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/app/components/ui/select'
import { Switch } from '@/app/components/ui/switch'
import { Settings as SettingsType } from '../types'
import { availableFonts } from '../types'

interface QuickSettingsProps {
  settings: SettingsType
  onUpdate: (updates: Partial<SettingsType>) => void
}

const languages = [
  { value: 'en', label: 'English' },
  { value: 'ar', label: 'العربية (Arabic)' },
  { value: 'es', label: 'Español (Spanish)' },
  { value: 'fr', label: 'Français (French)' },
  { value: 'de', label: 'Deutsch (German)' },
  { value: 'ru', label: 'Русский (Russian)' },
  { value: 'zh', label: '中文 (Chinese)' },
  { value: 'ja', label: '日本語 (Japanese)' },
  { value: 'ko', label: '한국어 (Korean)' },
  { value: 'pt', label: 'Português (Portuguese)' },
  { value: 'it', label: 'Italiano (Italian)' },
  { value: 'hi', label: 'हिन्दी (Hindi)' },
  { value: 'tr', label: 'Türkçe (Turkish)' },
  { value: 'nl', label: 'Nederlands (Dutch)' },
]

export function QuickSettings({ settings, onUpdate }: QuickSettingsProps) {
  return (
    <div className="p-4 border-b border-border/50">
      <div>
        <h3 className="text-base font-medium text-foreground mb-3 flex items-center gap-2">
          <Settings className="w-4 h-4" />
          Quick Settings
        </h3>

        <div className="space-y-3 text-foreground">
          <div>
            <label className="text-xs text-muted-foreground mb-1 block">Style</label>
            <Select
              value={settings.captionStyle}
              onValueChange={(value: 'karaoke' | 'oneliner' | 'vibrant') => onUpdate({ captionStyle: value })}
            >
              <SelectTrigger className="w-full h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="karaoke">Karaoke</SelectItem>
                <SelectItem value="oneliner">Oneliner</SelectItem>
                <SelectItem value="vibrant">Vibrant</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div>
            <label className="text-xs text-muted-foreground mb-1 block">Position</label>
            <Select
              value={settings.captionPosition}
              onValueChange={(value: 'bottom' | 'center') => onUpdate({ captionPosition: value })}
            >
              <SelectTrigger className="w-full h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="bottom">Bottom</SelectItem>
                <SelectItem value="center">Center</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div>
            <label className="text-xs text-muted-foreground mb-1 block">Language</label>
            <Select
              value={settings.selectedLanguage}
              onValueChange={(value) => onUpdate({ selectedLanguage: value })}
            >
              <SelectTrigger className="w-full h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {languages.map((lang) => (
                  <SelectItem key={lang.value} value={lang.value}>
                    {lang.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div>
            <label className="text-xs text-muted-foreground mb-1 block">Font</label>
            <Select value={settings.selectedFont} onValueChange={(value) => onUpdate({ selectedFont: value })}>
              <SelectTrigger className="w-full h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {availableFonts.map((font) => (
                  <SelectItem key={font.id} value={font.id}>
                    {font.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-center justify-between">
            <label className="text-xs text-muted-foreground">Glow Effect</label>
            <Switch checked={settings.glowEffect} onCheckedChange={(checked) => onUpdate({ glowEffect: checked })} />
          </div>
        </div>
      </div>
    </div>
  )
}

