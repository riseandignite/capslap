import { Zap } from 'lucide-react'
import { cn } from '@/lib/utils'
import { SettingsModal } from './SettingsModal'
import { TemplatesSection } from './TemplatesSection'
import { QuickSettings } from './QuickSettings'
import { ColorControls } from './ColorControls'
import { ExportFormats } from './ExportFormats'
import { ModelDownloader } from './ModelDownloader'
import { Template, Settings } from '../types'
import { ModelInfo } from '@/lib/preload'

interface SidebarProps {
  templates: Template[]
  settings: Settings
  apiKey: string
  isApiKeySettingsOpen: boolean
  onSelectTemplate: (template: Template) => void
  onUpdateSettings: (updates: Partial<Settings>) => void
  onToggleExportFormat: (formatId: string) => void
  onSaveApiKey: (apiKey: string) => void
  onOpenApiKeySettings: () => void
  onCloseApiKeySettings: (open: boolean) => void
}

export function Sidebar({
  templates,
  settings,
  apiKey,
  isApiKeySettingsOpen,
  onSelectTemplate,
  onUpdateSettings,
  onToggleExportFormat,
  onSaveApiKey,
  onOpenApiKeySettings,
  onCloseApiKeySettings,
}: SidebarProps) {
  return (
    <div
      className={cn(
        'bg-card/50 border-r border-border/50 flex flex-col transition-all duration-300 ease-out',
        'w-80'
      )}
    >
      <div className="p-4 pt-6 border-b border-border/50 relative">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 rounded-lg bg-gradient-to-br from-primary to-primary/80">
              <Zap className="w-6 h-6 text-primary-foreground" />
            </div>
            <h1 className="text-2xl font-bold text-primary">CapSlap</h1>
          </div>
          <SettingsModal
            onSave={onSaveApiKey}
            isOpen={isApiKeySettingsOpen}
            onOpenChange={onCloseApiKeySettings}
            apiKey={apiKey}
          />
        </div>
        <p className="text-sm text-muted-foreground truncate">Lightning-fast AI captions</p>
      </div>
      <div className={cn('sidebar-content flex-1 flex flex-col overflow-y-auto scrollbar-hide')}>
        <TemplatesSection templates={templates} settings={settings} onSelectTemplate={onSelectTemplate} />
        <QuickSettings settings={settings} onUpdate={onUpdateSettings} />
        <ColorControls settings={settings} onUpdate={onUpdateSettings} />
        <ExportFormats settings={settings} onToggleFormat={onToggleExportFormat} />
        <div className="p-4 border-b border-border/50">
          <ModelDownloader
            selectedModel={settings.selectedModel}
            onSelectModel={(model: ModelInfo['name']) => onUpdateSettings({ selectedModel: model })}
            apiKey={apiKey}
            onOpenApiKeySettings={onOpenApiKeySettings}
          />
        </div>
      </div>
    </div>
  )
}

