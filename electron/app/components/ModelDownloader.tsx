import React, { useState, useEffect } from 'react'
import { toast } from 'sonner'
import { Download } from 'lucide-react'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from './ui/tooltip'
import { ModelInfo, WhisperModel } from '@/lib/preload'

const MODEL_INFO: ModelInfo[] = [
  { name: 'whisper-1', size: 'Online', downloaded: true },
  { name: 'tiny', size: '75 MB', downloaded: false },
  { name: 'base', size: '147 MB', downloaded: false },
  { name: 'small', size: '488 MB', downloaded: false },
  { name: 'medium', size: '1.5 GB', downloaded: false },
  { name: 'large', size: '3.1 GB', downloaded: false },
]

export function ModelDownloader({
  selectedModel,
  onSelectModel,
  apiKey,
  onOpenApiKeySettings,
}: {
  selectedModel: string
  onSelectModel: (model: ModelInfo['name']) => void
  apiKey: string
  onOpenApiKeySettings: () => void
}) {
  const [models, setModels] = useState<ModelInfo[]>(MODEL_INFO)
  const [downloading, setDownloading] = useState<WhisperModel | null>(null)
  const [progress, setProgress] = useState(0)

  useEffect(() => {
    const checkModels = async () => {
      const updated = await Promise.all(
        MODEL_INFO.map(async (model) => {
          if (model.name === 'whisper-1') return model
          try {
            const exists = await window.rust.checkModelExists(model.name as WhisperModel)
            return { ...model, downloaded: exists }
          } catch {
            return model
          }
        })
      )
      setModels(updated)
    }
    checkModels()

    const unsubscribe = window.rust.onProgress((event: any) => {
      if (event.event === 'progress') {
        setProgress(Math.round(event.progress * 100))
      }
    })
    return () => unsubscribe()
  }, [])

  const handleDownload = async (modelName: WhisperModel, e: React.MouseEvent) => {
    e.stopPropagation()
    setDownloading(modelName)
    setProgress(0)

    try {
      await window.rust.downloadModel(modelName)
      setModels((prev) => prev.map((m) => (m.name === modelName ? { ...m, downloaded: true } : m)))
      toast.success(`${modelName} model downloaded successfully!`)
    } catch (error: any) {
      toast.error(`Failed to download ${modelName} model`)
    } finally {
      setDownloading(null)
      setProgress(0)
    }
  }

  const handleSelect = (model: ModelInfo) => {
    if (model.name === 'whisper-1' && !apiKey) {
      onOpenApiKeySettings()
      return
    }

    onSelectModel(model.name)
  }

  return (
    <TooltipProvider delayDuration={200}>
      <div>
        <h3 className="text-base font-medium text-foreground mb-3 flex items-center gap-2">
          <Download className="w-4 h-4" />
          AI Models
        </h3>

        <div className="space-y-2">
          {models.map((model) => {
            const isSelected = selectedModel === model.name
            const canSelect = model.name === 'whisper-1' || model.downloaded
            const isDownloading = downloading === model.name

            return (
              <Tooltip key={model.name}>
                <TooltipTrigger asChild>
                  <div
                    onClick={() => canSelect && !isDownloading && handleSelect(model)}
                    className={`
                      group relative flex items-center justify-between p-3 border rounded-lg transition-all duration-200
                      ${isSelected ? 'border-primary/70 bg-primary/10 shadow-sm' : 'border-border/50'}
                      ${
                        canSelect && !isDownloading
                          ? 'cursor-pointer hover:border-primary/70'
                          : isDownloading
                            ? 'cursor-wait'
                            : 'cursor-not-allowed opacity-70'
                      }
                    `}
                  >
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-xs font-medium capitalize text-foreground">{model.name}</span>
                      </div>
                      <div className="flex items-center gap-2 mt-1">
                        <span className="text-[10px] text-muted-foreground">{model.size}</span>
                        {model.downloaded && model.name !== 'whisper-1' && (
                          <span className="text-[10px] text-green-600 flex items-center gap-1">Downloaded</span>
                        )}
                      </div>
                    </div>

                    {isDownloading && (
                      <div className="w-32">
                        <div className="text-[10px] text-center mb-1 font-medium text-foreground">{progress}%</div>
                        <div className="h-1.5 bg-secondary/50 rounded-full overflow-hidden">
                          <div
                            className="h-full bg-primary rounded-full transition-all duration-300"
                            style={{ width: `${progress}%` }}
                          />
                        </div>
                      </div>
                    )}

                    {!canSelect && !isDownloading && (
                      <div
                        onClick={(e) => handleDownload(model.name, e)}
                        className="opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer hover:text-primary"
                      >
                        <Download className="w-4 h-4 text-muted-foreground hover:text-primary transition-colors" />
                      </div>
                    )}
                  </div>
                </TooltipTrigger>
                <TooltipContent
                  side="right"
                  sideOffset={8}
                  className="bg-card/95 backdrop-blur-sm border border-primary/20 text-foreground shadow-xl max-w-xs rounded-sm"
                >
                  <p className="text-xs font-medium">
                    {model.name === 'whisper-1'
                      ? apiKey
                        ? 'Uses OpenAI API for transcription'
                        : 'Requires API key - click to configure'
                      : model.downloaded
                        ? 'Click to use this model'
                        : `Download ${model.name} model to use`}
                  </p>
                </TooltipContent>
              </Tooltip>
            )
          })}
        </div>
      </div>
    </TooltipProvider>
  )
}
