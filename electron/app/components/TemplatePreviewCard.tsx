import React from 'react'
import { cn } from '@/lib/utils'
import { Template } from '../types'

interface TemplatePreviewCardProps {
  template: Template
  isSelected: boolean
  onSelect: () => void
}

export function TemplatePreviewCard({ template, isSelected, onSelect }: TemplatePreviewCardProps) {
  const [isHovered, setIsHovered] = React.useState(false)

  return (
    <div
      className={cn(
        'group relative rounded-lg border transition-all duration-200 cursor-pointer overflow-hidden',
        'hover:scale-[1.02]',
        isSelected
          ? 'border-primary bg-primary/10 ring-1 ring-primary/20'
          : 'border-border/50 bg-card/50 hover:border-primary/50 hover:bg-primary/5'
      )}
      onClick={onSelect}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
    >
      <div className="flex flex-col h-full">
        <div className="relative w-full" style={{ aspectRatio: '9/16' }}>
          <video
            src={template.src || ''}
            className="w-full h-full object-cover"
            muted
            loop
            playsInline
            ref={(el) => {
              if (el) {
                if (isHovered) {
                  el.play()
                } else {
                  el.pause()
                  el.currentTime = 0
                }
              }
            }}
          />
          <div className="absolute inset-0 bg-gradient-to-t from-black/60 via-transparent to-transparent" />
          <div className="absolute bottom-0 left-0 right-0 p-2">
            <h3 className="font-medium text-white text-xs">{template.name}</h3>
          </div>
        </div>
      </div>

      {isSelected && (
        <div className="absolute top-3 right-3">
          <div className="w-2 h-2 rounded-full bg-primary"></div>
        </div>
      )}
    </div>
  )
}

