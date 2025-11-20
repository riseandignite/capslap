import { Film } from 'lucide-react'
import { TemplatePreviewCard } from './TemplatePreviewCard'
import { Template, Settings } from '../types'

interface TemplatesSectionProps {
  templates: Template[]
  settings: Settings
  onSelectTemplate: (template: Template) => void
}

export function TemplatesSection({ templates, settings, onSelectTemplate }: TemplatesSectionProps) {
  return (
    <div className="p-4 border-b border-border/50">
      <h3 className="text-base font-medium text-foreground mb-3 flex items-center gap-2">
        <Film className="w-4 h-4" />
        Templates
      </h3>
      <div className="grid grid-cols-3 gap-2">
        {templates.map((template) => (
          <TemplatePreviewCard
            key={template.id}
            template={template}
            isSelected={settings.selectedTemplate === template.id}
            onSelect={() => onSelectTemplate(template)}
          />
        ))}
      </div>
    </div>
  )
}

