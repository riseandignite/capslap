import React, { useEffect, useState } from 'react'
import { Button } from '@/app/components/ui/button'
import { toast, Toaster } from 'sonner'
import { Switch } from '@/app/components/ui/switch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/app/components/ui/select'
import { Upload, Film, Download, Cog, Trash, Zap, Settings, FileVideo, Key, Palette } from 'lucide-react'
import { cn } from '@/lib/utils'
import { TitleBar } from './components/TitleBar'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogFooter,
  DialogClose,
} from '@/app/components/ui/dialog'
import { Input } from './components/ui/input'

const getVideoPath = (filename: string) => {
  if (import.meta.env.DEV) {
    return `./assets/${filename}`
  }

  return `res://videos/${filename}`
}

interface Template {
  id: 'oneliner' | 'karaoke' | 'vibrant'
  captionStyle: 'karaoke' | 'oneliner' | 'vibrant'
  name: string
  src: string | null
  textColor: string
  highlightWordColor: string
  outlineColor: string
  glowEffect: boolean
  font: string
  position: 'bottom' | 'center'
}

interface Settings {
  selectedTemplate: Template['id']
  exportFormats: string[]
  selectedFont: string
  textColor: string
  highlightWordColor: string
  outlineColor: string
  glowEffect: boolean
  captionStyle: Template['captionStyle']
  captionPosition: 'bottom' | 'center'
}

const defaultSettings: Settings = {
  selectedTemplate: 'karaoke',
  exportFormats: ['9:16'],
  selectedFont: 'montserrat-black',
  textColor: '#ffffff',
  highlightWordColor: '#ffff00',
  outlineColor: '#000000',
  glowEffect: false,
  captionStyle: 'karaoke',
  captionPosition: 'bottom',
}

const templates: Template[] = [
  {
    id: 'oneliner',
    captionStyle: 'oneliner',
    name: 'Oneliner',
    src: getVideoPath('oneliner.mp4'),
    textColor: '#ffffff',
    highlightWordColor: '#ffff00',
    outlineColor: '#000000',
    glowEffect: true,
    font: 'montserrat-black',
    position: 'bottom',
  },
  {
    id: 'karaoke',
    captionStyle: 'karaoke',
    name: 'Karaoke',
    src: getVideoPath('karaoke.mp4'),
    textColor: '#ffffff',
    highlightWordColor: '#00f924',
    outlineColor: '#000000',
    glowEffect: false,
    font: 'komika-axis',
    position: 'bottom',
  },
  {
    id: 'vibrant',
    captionStyle: 'vibrant',
    name: 'Vibrant',
    src: getVideoPath('vibrant.mp4'),
    textColor: '#898284',
    highlightWordColor: '#7ef1c5',
    outlineColor: '#000000',
    glowEffect: false,
    font: 'roboto-bold',
    position: 'center',
  },
]

const availableExportFormats = [
  { id: '9:16', name: '9:16', description: 'Perfect for TikTok, Instagram Stories, YouTube Shorts' },
  { id: '16:9', name: '16:9', description: 'Standard for YouTube, desktop viewing' },
  { id: '1:1', name: '1:1', description: 'Instagram posts, Facebook' },
  { id: '4:5', name: '4:5', description: 'Instagram feed posts' },
]

const availableFonts = [
  { id: 'komika-axis', name: 'Komika Axis' },
  { id: 'montserrat-black', name: 'Montserrat Black' },
  { id: 'theboldfont', name: 'THEBOLDFONT' },
  { id: 'kanit-bold', name: 'Kanit Bold' },
  { id: 'poppins-black', name: 'Poppins Black' },
  { id: 'oswald-bold', name: 'Oswald Bold' },
  { id: 'bangers-regular', name: 'Bangers Regular' },
  { id: 'worksans-bold', name: 'WorkSans Bold' },
  { id: 'roboto-bold', name: 'Roboto Bold' },
]

const FONT_NAMES = {
  'montserrat-black': 'Montserrat Black',
  'komika-axis': 'Komika Axis',
  theboldfont: 'THEBOLDFONT',
  'kanit-bold': 'Kanit Bold',
  'poppins-black': 'Poppins Black',
  'oswald-bold': 'Oswald Bold',
  'bangers-regular': 'Bangers Regular',
  'worksans-bold': 'WorkSans Bold',
  'roboto-bold': 'Roboto Bold',
} as const

const getFontName = (fontId: string): string => {
  return FONT_NAMES[fontId as keyof typeof FONT_NAMES] || 'Montserrat Black'
}

function TemplatePreviewCard({
  template,
  isSelected,
  onSelect,
}: {
  template: Template
  isSelected: boolean
  onSelect: () => void
}) {
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
        {/* Video Preview with 9:16 aspect ratio */}
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

          {/* Overlay for better text visibility */}
          <div className="absolute inset-0 bg-gradient-to-t from-black/60 via-transparent to-transparent" />

          {/* Video info overlay */}
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

function SettingsModal({
  onSave,
  isOpen,
  onOpenChange,
  apiKey,
}: {
  onSave: (apiKey: string) => void
  isOpen: boolean
  onOpenChange: (open: boolean) => void
  apiKey: string
}) {
  const [apiKeyState, setApiKeyState] = useState(apiKey || '')

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="w-9 h-9 text-muted-foreground hover:text-foreground bg-card/80 backdrop-blur-sm border border-border/50 hover:bg-card/90 transition-all duration-200 focus:ring-0 focus-visible:ring-0 ring-0"
        >
          <Key className="!w-3.5 !h-3.5" />
        </Button>
      </DialogTrigger>

      <DialogContent className="max-w-[450px]">
        <DialogHeader>
          <DialogTitle className="text-xl font-medium text-white/90">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-lg bg-primary/20">
                <Key className="w-5 h-5 text-primary" />
              </div>
              <h2 className="text-xl font-medium text-foreground">API Settings</h2>
            </div>
          </DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-4 py-4">
          <div className="grid gap-2">
            <label htmlFor="name" className="text-sm text-white/40 font-medium">
              OpenAI Api Key
            </label>
            <Input
              id="key"
              placeholder="sk-..."
              className="w-full px-4 py-3 border border-border rounded-lg bg-background/50 focus:outline-none hover:border-primary/50 focus:border-primary/70 ring-0 focus-visible:ring-2 focus-visible:ring-primary/20 focus:ring-2 focus:ring-primary/20 text-foreground transition-all duration-200"
              value={apiKeyState}
              onChange={(e) => setApiKeyState(e.target.value)}
            />
          </div>
        </div>

        <DialogFooter className="gap-4">
          <DialogClose asChild>
            <Button variant="outline" className="flex-1 bg-transparent text-foreground">
              Cancel
            </Button>
          </DialogClose>
          <DialogClose asChild>
            <Button
              onClick={() => {
                onSave(apiKeyState)
                onOpenChange(false)
              }}
              className="flex-1 bg-primary text-primary-foreground hover:bg-primary/90"
            >
              Save Settings
            </Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function FileCard({ path, onRemove }: { path: string; onRemove: () => void }) {
  const fileName = path.split('/').pop() || ''

  return (
    <div className="group relative bg-gradient-to-br from-card/80 to-card/40 border border-border/30 rounded-xl p-4 hover:border-primary/40 transition-all duration-300 backdrop-blur-sm">
      <div className="flex items-center gap-4">
        <div className="relative">
          <FileVideo className="w-6 h-6 text-white" />
        </div>

        <div className="flex-1 min-w-0">
          <p className="font-medium text-foreground truncate text-sm">{fileName}</p>
        </div>

        <Button
          variant="ghost"
          size="sm"
          className="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-destructive hover:!bg-destructive/10 transition-all duration-200 rounded-lg"
          onClick={onRemove}
        >
          <Trash className="w-4 h-4" />
        </Button>
      </div>
    </div>
  )
}

export default function App() {
  const [selectedVideos, setSelectedVideos] = useState<string[]>([])
  const [videoSettings, setVideoSettings] = useState<Settings>(defaultSettings)
  const [apiKey, setApiKey] = useState('')
  const [isLoaded, setIsLoaded] = useState(false)
  const [isGenerating, setIsGenerating] = useState(false)
  const [isDragOver, setIsDragOver] = useState(false)
  const [_, setDragCounter] = useState(0)
  const [isApiKeySettingsOpen, setIsApiKeySettingsOpen] = useState(false)
  const [shouldGenerateAfterApiKey, setShouldGenerateAfterApiKey] = useState(false)

  useEffect(() => {
    const savedSettings = localStorage.getItem('settings-v1')
    const savedApiKey = localStorage.getItem('api-key-v1')

    if (savedApiKey) {
      setApiKey(savedApiKey)
    }

    if (savedSettings) {
      const cachedVideoSettings: Settings = JSON.parse(savedSettings)
      setVideoSettings(cachedVideoSettings)
    }
    setIsLoaded(true)
  }, [])

  useEffect(() => {
    if (!isLoaded) return
    localStorage.setItem('settings-v1', JSON.stringify(videoSettings))
  }, [videoSettings, isLoaded])

  const updateSettings = (updates: Partial<Settings>) => {
    setVideoSettings((prev) => ({ ...prev, ...updates }))
  }

  const selectTemplate = (template: Template) => {
    updateSettings({
      selectedTemplate: template.id,
      captionStyle: template.captionStyle,
      textColor: template.textColor,
      highlightWordColor: template.highlightWordColor,
      outlineColor: template.outlineColor,
      glowEffect: template.glowEffect,
      selectedFont: template.font,
      captionPosition: template.position,
    })
  }

  const handleVideoSelect = async () => {
    try {
      const paths = await window.rust.openFiles?.([{ name: 'Video Files', extensions: ['mp4', 'mov', 'mkv', 'avi'] }])

      if (paths && paths.length > 0) {
        const duplicates = paths.filter((path) => selectedVideos.includes(path))
        const pathsWithoutDuplicates = paths.filter((path) => !selectedVideos.includes(path))

        if (duplicates.length > 0) {
          toast.error('Video already uploaded', {
            description: `Duplicate videos: ${duplicates.join(', ')}`,
          })
        }

        if (pathsWithoutDuplicates.length === 0) {
          return
        }

        setSelectedVideos((prev) => [...prev, ...pathsWithoutDuplicates])
      }
    } catch (error) {
      console.error('File picker error:', error)
    }
  }

  const handleExportFormatToggle = (formatId: string) => {
    setVideoSettings((prev) => ({
      ...prev,
      exportFormats: prev.exportFormats.includes(formatId)
        ? prev.exportFormats.filter((f) => f !== formatId)
        : [...prev.exportFormats, formatId],
    }))
  }

  const handleSaveApiKey = (apiKey: string) => {
    setApiKey(apiKey.trim())
    localStorage.setItem('api-key-v1', apiKey.trim())

    if (shouldGenerateAfterApiKey) {
      setShouldGenerateAfterApiKey(false)
      handleGenerate()
    }
  }

  const handleGenerate = async () => {
    if (!selectedVideos.length) {
      toast.error('Please select a video first')
      return
    }

    if (!apiKey) {
      setShouldGenerateAfterApiKey(true)
      setIsApiKeySettingsOpen(true)
      return
    }

    try {
      setIsGenerating(true)

      const results = await Promise.allSettled(
        selectedVideos.map(async (video) =>
          window.rust.call('generateCaptions', {
            inputVideo: video,
            exportFormats: videoSettings.exportFormats,
            karaoke: videoSettings.captionStyle === 'karaoke',
            fontName: getFontName(videoSettings.selectedFont),
            splitByWords: true,
            model: 'whisper-1',
            language: null,
            prompt: null,
            textColor: videoSettings.textColor,
            highlightWordColor: videoSettings.highlightWordColor,
            outlineColor: videoSettings.outlineColor,
            glowEffect: videoSettings.glowEffect,
            position: videoSettings.captionPosition,
            apiKey: apiKey,
          })
        )
      )

      const successful = results.filter((r) => r.status === 'fulfilled')
      const failed = results.filter((r) => r.status === 'rejected')

      if (successful.length > 0) {
        toast.success(`Generated ${successful.length} videos successfully!`)
      }

      if (failed.length > 0) {
        toast.error(`Failed to generate ${failed.length} videos`, {
          description: failed.map((f) => f.reason).join(', '),
        })
      }
    } catch (error: any) {
      console.error('Generation error:', error)
      toast.error('Error generating video', {
        description: error.message || 'An unexpected error occurred',
      })
    } finally {
      setIsGenerating(false)
    }
  }

  const handleDragEnter = (e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setDragCounter((prev) => prev + 1)
    setIsDragOver(true)
  }

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
  }

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setDragCounter((prev) => {
      const newCount = prev - 1
      if (newCount <= 0) {
        setIsDragOver(false)
        return 0
      }
      return newCount
    })
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setIsDragOver(false)
    setDragCounter(0)

    const files = Array.from(e.dataTransfer.files)

    if (files.length > 0) {
      const supportedExtensions = ['mp4', 'mov', 'mkv', 'avi']

      const validFiles = files.filter((file) => {
        const fileExtension = file.name.split('.').pop()?.toLowerCase()
        return fileExtension && supportedExtensions.includes(fileExtension)
      })
      const invalidFiles = files.filter((file) => {
        const fileExtension = file.name.split('.').pop()?.toLowerCase()
        return fileExtension && !supportedExtensions.includes(fileExtension)
      })

      if (invalidFiles.length > 0) {
        toast.error(`Unsupported files format:`, {
          description: `Supported: ${supportedExtensions.join(', ')}`,
        })
      }

      const filesPaths = validFiles.map((file) => window.rust.getFilePath(file)).filter((path) => path !== null)

      if (filesPaths && filesPaths.length > 0) {
        const duplicates = filesPaths.filter((path) => selectedVideos.includes(path))
        const pathsWithoutDuplicates = filesPaths.filter((path) => !selectedVideos.includes(path))

        if (duplicates.length > 0) {
          toast.error('Video already uploaded', {
            description: `Duplicate videos: ${duplicates.join(', ')}`,
          })
        }

        if (pathsWithoutDuplicates.length === 0) {
          return
        }

        setSelectedVideos((prev) => [...prev, ...pathsWithoutDuplicates.filter((path) => path !== null)])
      }
    }
  }

  return (
    <>
      <TitleBar />
      <div
        className="pt-9 relative flex h-full overflow-hidden"
        onDragEnter={handleDragEnter}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <Toaster />

        {isDragOver && (
          <div className="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
            <div className="absolute inset-0 bg-[#08090a] backdrop-blur-md animate-in fade-in duration-300" />

            <div className="relative z-10 flex flex-col items-center justify-center p-12 rounded-3xl border-2 border-dashed border-primary/60 bg-gradient-to-br from-primary/20 via-primary/10 to-primary/20 backdrop-blur-xl animate-in zoom-in-95 fade-in duration-300 shadow-2xl">
              <div className="relative mb-8">
                <div className="absolute inset-0 rounded-full bg-primary/20 blur-xl animate-pulse" />
                <div className="relative p-4 rounded-full bg-primary/20 border border-primary/30">
                  <Upload className="w-12 h-12 text-primary" />
                </div>
              </div>

              <div className="text-center space-y-3">
                <h3 className="text-2xl font-medium text-primary">Drop your video to upload</h3>
                <div className="flex items-center justify-center gap-2 text-primary/70 text-sm">
                  <span>Supported:</span>
                  <div className="flex gap-1">
                    {['MP4', 'MOV', 'MKV', 'AVI'].map((format) => (
                      <span key={format} className="px-1.5 py-0.5 bg-primary/20 rounded text-xs font-mono">
                        {format}
                      </span>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Sidebar */}
        <div
          className={cn(
            'bg-card/50 border-r border-border/50 flex flex-col transition-all duration-300 ease-out',
            'w-80'
          )}
        >
          {/* Header */}
          <div className="p-4 pt-6 border-b border-border/50 relative">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3 mb-2">
                <div className="p-2 rounded-lg bg-gradient-to-br from-primary to-primary/80">
                  <Zap className="w-6 h-6 text-primary-foreground" />
                </div>
                <h1 className="text-2xl font-bold text-primary">CapSlap</h1>
              </div>
              <SettingsModal
                onSave={handleSaveApiKey}
                isOpen={isApiKeySettingsOpen}
                onOpenChange={setIsApiKeySettingsOpen}
                apiKey={apiKey}
              />
            </div>
            <p className="text-sm text-muted-foreground truncate">Lightning-fast AI captions</p>
          </div>
          <div className={cn('sidebar-content flex-1 flex flex-col overflow-y-auto scrollbar-hide')}>
            {/* Templates */}
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
                    isSelected={videoSettings.selectedTemplate === template.id}
                    onSelect={() => selectTemplate(template)}
                  />
                ))}
              </div>
            </div>

            {/* Quick Settings */}
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
                      value={videoSettings.captionStyle}
                      onValueChange={(value: 'karaoke' | 'oneliner') => updateSettings({ captionStyle: value })}
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
                      value={videoSettings.captionPosition}
                      onValueChange={(value: 'bottom' | 'center') => updateSettings({ captionPosition: value })}
                    >
                      <SelectTrigger className="w-full  h-8 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="bottom">Bottom</SelectItem>
                        <SelectItem value="center">Center</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div>
                    <label className="text-xs text-muted-foreground mb-1 block">Font</label>
                    <Select
                      value={videoSettings.selectedFont}
                      onValueChange={(value) => updateSettings({ selectedFont: value })}
                    >
                      <SelectTrigger className="w-full  h-8 text-xs">
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
                  {/* </div> */}

                  <div className="flex items-center justify-between">
                    <label className="text-xs text-muted-foreground">Glow Effect</label>
                    <Switch
                      checked={videoSettings.glowEffect}
                      onCheckedChange={(checked) => updateSettings({ glowEffect: checked })}
                    />
                  </div>
                </div>
              </div>
            </div>

            {/* Color Controls */}
            <div className="p-4 border-b border-border/50">
              <h3 className="text-base font-medium text-foreground mb-3 flex items-center gap-2">
                <Palette className="w-4 h-4" />
                Color Controls
              </h3>
              <div className="space-y-3">
                <div className="space-y-3">
                  <label className="text-xs text-muted-foreground mb-1 block">Text Color</label>
                  <div className="flex items-center gap-3">
                    <div className="relative">
                      <input
                        type="color"
                        value={videoSettings.textColor}
                        onChange={(e) => updateSettings({ textColor: e.target.value })}
                        className="w-12 h-8 rounded-sm border border-border bg-transparent cursor-pointer overflow-hidden opacity-0 absolute inset-0"
                      />
                      <div
                        className="w-12 h-8 rounded-sm border border-border cursor-pointer transition-all duration-200 hover:scale-105 hover:border-primary/50"
                        style={{ backgroundColor: videoSettings.textColor }}
                      />
                    </div>
                    <div className="flex-1">
                      <Input
                        type="text"
                        value={videoSettings.textColor}
                        onChange={(e) => {
                          const value = e.target.value
                          if (/^#[0-9A-Fa-f]{0,6}$/.test(value) || value === '') {
                            updateSettings({ textColor: value })
                          }
                        }}
                        onBlur={(e) => {
                          let value = e.target.value
                          if (value && !value.startsWith('#')) value = '#' + value
                          if (/^#[0-9A-Fa-f]{6}$/.test(value)) {
                            updateSettings({ textColor: value })
                          } else if (value.length > 0) {
                            updateSettings({ textColor: videoSettings.textColor })
                          }
                        }}
                        className="w-full h-8 px-2 py-1.5 border border-border rounded-sm bg-background/50 focus:outline-none hover:border-primary/50 focus:border-primary/70 ring-0 focus-visible:ring-0 focus:ring-0 text-foreground transition-all duration-200"
                        placeholder="#ffffff"
                        maxLength={7}
                      />
                    </div>
                  </div>
                </div>

                <div className="">
                  <label className="text-xs text-muted-foreground mb-1 block">Highlight Color</label>
                  <div className="flex items-center gap-3">
                    <div className="relative">
                      <input
                        type="color"
                        value={videoSettings.highlightWordColor}
                        onChange={(e) => updateSettings({ highlightWordColor: e.target.value })}
                        className="w-12 h-8 rounded-sm border border-border bg-transparent cursor-pointer overflow-hidden opacity-0 absolute inset-0"
                      />
                      <div
                        className="w-12 h-8 rounded-sm border border-border cursor-pointer transition-all duration-200 hover:scale-105 hover:border-primary/50"
                        style={{ backgroundColor: videoSettings.highlightWordColor }}
                      />
                    </div>
                    <div className="flex-1">
                      <Input
                        type="text"
                        value={videoSettings.highlightWordColor}
                        onChange={(e) => {
                          const value = e.target.value
                          if (/^#[0-9A-Fa-f]{0,6}$/.test(value) || value === '') {
                            updateSettings({ highlightWordColor: value })
                          }
                        }}
                        onBlur={(e) => {
                          let value = e.target.value
                          if (value && !value.startsWith('#')) value = '#' + value
                          if (/^#[0-9A-Fa-f]{6}$/.test(value)) {
                            updateSettings({ highlightWordColor: value })
                          } else if (value.length > 0) {
                            updateSettings({ highlightWordColor: videoSettings.highlightWordColor })
                          }
                        }}
                        className="w-full h-8 px-2 py-1.5 border border-border rounded-sm bg-background/50 focus:outline-none hover:border-primary/50 focus:border-primary/70 ring-0 focus-visible:ring-0 focus:ring-0 text-foreground transition-all duration-200"
                        placeholder="#ffffff"
                        maxLength={7}
                      />
                    </div>
                  </div>
                </div>

                <div className="">
                  <label className="text-xs text-muted-foreground mb-1 block">Outline Color</label>
                  <div className="flex items-center gap-3">
                    <div className="relative">
                      <input
                        type="color"
                        value={videoSettings.outlineColor}
                        onChange={(e) => updateSettings({ outlineColor: e.target.value })}
                        className="w-12 h-8 rounded-sm border border-border bg-transparent cursor-pointer overflow-hidden opacity-0 absolute inset-0"
                      />
                      <div
                        className="w-12 h-8 rounded-sm border border-border cursor-pointer transition-all duration-200 hover:scale-105 hover:border-primary/50"
                        style={{ backgroundColor: videoSettings.outlineColor }}
                      />
                    </div>
                    <div className="flex-1">
                      <Input
                        type="text"
                        value={videoSettings.outlineColor}
                        onChange={(e) => {
                          const value = e.target.value
                          if (/^#[0-9A-Fa-f]{0,6}$/.test(value) || value === '') {
                            updateSettings({ outlineColor: value })
                          }
                        }}
                        onBlur={(e) => {
                          let value = e.target.value
                          if (value && !value.startsWith('#')) value = '#' + value
                          if (/^#[0-9A-Fa-f]{6}$/.test(value)) {
                            updateSettings({ outlineColor: value })
                          } else if (value.length > 0) {
                            updateSettings({ outlineColor: videoSettings.outlineColor })
                          }
                        }}
                        className="w-full h-8 px-2 py-1.5 border border-border rounded-sm bg-background/50 focus:outline-none hover:border-primary/50 focus:border-primary/70 ring-0 focus-visible:ring-0 focus:ring-0 text-foreground transition-all duration-200"
                        placeholder="#ffffff"
                        maxLength={7}
                      />
                    </div>
                  </div>
                </div>
              </div>
            </div>

            {/* Export Formats */}
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
                        videoSettings.exportFormats?.includes(format.id)
                          ? 'border-primary/60 text-primary'
                          : 'border-border/50 text-muted-foreground hover:border-primary/50'
                      )}
                      onClick={() => handleExportFormatToggle(format.id)}
                    >
                      {format.name}
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>
        </div>
        {/* Upload sectino */}

        <div className="relative flex-1 flex flex-col">
          <div className="relative flex-1 overflow-y-auto scrollbar-hide pb-28">
            {selectedVideos.length === 0 && (
              <div className="h-full px-8 pt-6">
                <div
                  className={cn(
                    'group max-w-2xl mx-auto relative h-full flex flex-col items-center justify-center p-12 rounded-2xl border-2 border-dashed transition-all duration-300 cursor-pointer',
                    'hover:border-primary/60 hover:bg-primary/5',
                    isDragOver ? 'border-primary bg-primary/10 scale-[1.01]' : 'border-border/50 bg-card/30'
                  )}
                  onClick={handleVideoSelect}
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
            )}

            {selectedVideos.length > 0 && (
              <div className="flex justify-center w-full px-8 mx-auto">
                <div className="space-y-4 w-full max-w-2xl">
                  <div className="flex items-center justify-between sticky top-0 bg-background/80 backdrop-blur-xs z-10 rounded-lg py-2 pt-6">
                    <h3 className="text-lg font-medium text-foreground flex items-center gap-2">Uploaded files</h3>
                    <div className="flex items-center gap-2">
                      {selectedVideos.length > 1 && (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setSelectedVideos([])}
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
                      selectedVideos.length <= 4 ? 'grid-cols-1' : 'grid-cols-1 lg:grid-cols-2'
                    )}
                  >
                    {selectedVideos.map((path, index) => (
                      <FileCard
                        key={index}
                        path={path}
                        onRemove={() => setSelectedVideos((prev) => prev.filter((p) => p !== path))}
                      />
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>

          <div className="flex items-center justify-center w-full py-6 border-t border-border/50 absolute bottom-0 bg-background/80 backdrop-blur-xs z-10 px-8">
            <Button
              onClick={handleGenerate}
              disabled={!selectedVideos.length || !videoSettings.exportFormats?.length || isGenerating}
              size="lg"
              className="max-w-2xl w-full py-4 text-lg font-medium bg-primary text-primary-foreground disabled:opacity-50 disabled:scale-100 transition-all duration-300"
            >
              {isGenerating ? (
                <>
                  <Cog className="w-5 h-5 animate-spin mr-2" />
                  Generating...
                </>
              ) : selectedVideos.length > 0 ? (
                <>
                  <Zap className="w-5 h-5 mr-2" />
                  Generate {selectedVideos.length * videoSettings.exportFormats?.length} video
                  {selectedVideos.length * videoSettings.exportFormats?.length > 1 ? 's' : ''}
                </>
              ) : (
                <>
                  <Zap className="w-5 h-5 mr-2" />
                  Upload videos first
                </>
              )}
            </Button>
          </div>
        </div>
      </div>
    </>
  )
}
