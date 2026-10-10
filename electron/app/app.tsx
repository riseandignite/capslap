import React, { useEffect, useState } from 'react'
import { Button } from '@/app/components/ui/button'
import { toast, Toaster } from 'sonner'
import { Switch } from '@/app/components/ui/switch'
import { Slider } from '@/app/components/ui/slider'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/app/components/ui/select'
import {
  Upload,
  Film,
  Download,
  Cog,
  Trash,
  Zap,
  Settings,
  FileVideo,
  Key,
  Palette,
  ChevronDown,
  Check,
  Loader2,
} from 'lucide-react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuSub,
  DropdownMenuSubTrigger,
  DropdownMenuSubContent,
} from '@/app/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { TitleBar } from './components/TitleBar'
import { ModelDownloader } from './components/ModelDownloader'
import { CaptionEditor, CaptionSegment } from './components/CaptionEditor'
import type { PositionOverride } from './components/CaptionPositionPanel'
import { RecentVideos, rememberRecentVideos } from './components/RecentVideos'
import { blockedBandsFor, loadPlatformSelection, savePlatformSelection, type PlatformId } from './components/safe-areas'
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
import { ModelInfo } from '@/lib/preload'

const getVideoPath = (filename: string) => {
  if (import.meta.env.DEV) {
    return `./assets/${filename}`
  }

  return `res://videos/${filename}`
}

// Function to display user-friendly errors
const showErrorToast = (error: any, count: number = 1) => {
  const countText = count > 1 ? ` (${count} videos)` : ''

  switch (error.name) {
    case 'API_KEY_MISSING':
      toast.error('🔑 API Key Not Configured', {
        description: 'Add OpenAI API key in settings for better transcription quality.',
        action: {
          label: 'Settings',
          onClick: () => {
            // Will open API key settings
          },
        },
      })
      break

    case 'API_KEY_INVALID':
      toast.error('🔑 Invalid API Key', {
        description: 'Please check your OpenAI API key in settings.',
      })
      break

    case 'NO_LOCAL_MODELS':
      toast.warning('📥 Using Online Transcription', {
        description: 'Local models not found. Transcription performed via OpenAI API.',
      })
      break

    case 'BINARY_NOT_FOUND':
      toast.error('⚙️ System Error', {
        description: 'Application components not found. Try reinstalling CapSlap.',
      })
      break

    case 'BINARY_DEP_MISSING':
      toast.error('🧩 Missing System Libraries', {
        description: 'Media tools require static ffmpeg/ffprobe builds. Please reinstall or contact support.',
      })
      break

    case 'NETWORK_ERROR':
      toast.error('🌐 Internet Connection Problem', {
        description: 'Check your network connection and try again.',
      })
      break

    case 'RATE_LIMIT':
      toast.error('⏰ Rate Limit Exceeded', {
        description: 'Too many requests to OpenAI API. Try again later.',
      })
      break

    case 'QUOTA_EXCEEDED':
      toast.error('💳 API Quota Exhausted', {
        description: 'Top up your OpenAI balance or use local models.',
      })
      break

    case 'FILE_NOT_FOUND':
      toast.error('📁 File Not Found', {
        description: 'Make sure the video file exists and is accessible.',
      })
      break

    default:
      toast.error(`❌ Error${countText}`, {
        description: error.message || 'An unexpected error occurred. Please try again.',
      })
  }
}

interface Template {
  id: 'oneliner' | 'karaoke' | 'karaoke-multiline' | 'vibrant' | 'storyteller'
  captionStyle: 'karaoke' | 'karaoke-multiline' | 'oneliner' | 'vibrant' | 'storyteller'
  name: string
  src: string | null
  textColor: string
  highlightWordColor: string
  outlineColor: string
  glowEffect: boolean
  font: string
  position: 'top' | 'top-quarter' | 'center' | 'bottom-quarter' | 'bottom'
}

interface Settings {
  selectedTemplate: Template['id']
  exportFormats: string[]
  selectedFont: string
  selectedModel: ModelInfo['name']
  textColor: string
  highlightWordColor: string
  outlineColor: string
  glowEffect: boolean
  captionStyle: Template['captionStyle']
  captionPosition: 'top' | 'top-quarter' | 'center' | 'bottom-quarter' | 'bottom'
  selectedLanguage: string
  outputSize?: string
  cropStrategy?: string
  fontSize: number
  /** Optional base URL of an OpenAI-compatible Whisper server (e.g. whisper.cpp server on the local network) */
  whisperServerUrl: string
}

const defaultSettings: Settings = {
  selectedTemplate: 'karaoke',
  exportFormats: ['9:16'],
  selectedFont: 'montserrat-black',
  selectedModel: 'whisper-1',
  textColor: '#ffffff',
  highlightWordColor: '#ffff00',
  outlineColor: '#000000',
  glowEffect: false,
  captionStyle: 'karaoke',
  captionPosition: 'center',
  selectedLanguage: 'auto',
  outputSize: 'original',
  cropStrategy: 'fit',
  fontSize: 65,
  whisperServerUrl: '',
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
  {
    id: 'storyteller',
    captionStyle: 'storyteller',
    name: 'Storyteller',
    src: getVideoPath('vibrant.mp4'), // Placeholder until we have a specific one
    textColor: '#ffffff',
    highlightWordColor: '#ffff00',
    outlineColor: '#000000',
    glowEffect: true,
    font: 'montserrat-black',
    position: 'center',
  },
]

const availableExportFormats = [
  { id: '9:16', name: '9:16', description: 'Perfect for TikTok, Instagram Stories, YouTube Shorts' },
  { id: '16:9', name: '16:9', description: 'Standard for YouTube, desktop viewing' },
  { id: '1:1', name: '1:1', description: 'Instagram posts, Facebook' },
  { id: '4:5', name: '4:5', description: 'Instagram feed posts' },
]

const fontCategories = [
  {
    name: 'Modern / Sans',
    fonts: [
      { id: 'montserrat-black', name: 'Montserrat Black' },
      { id: 'roboto-bold', name: 'Roboto Bold' },
      { id: 'open-sans', name: 'Open Sans' },
      { id: 'lato', name: 'Lato' },
      { id: 'raleway', name: 'Raleway' },
      { id: 'kanit-bold', name: 'Kanit Bold' },
      { id: 'poppins-black', name: 'Poppins Black' },
      { id: 'worksans-bold', name: 'WorkSans Bold' },
    ],
  },
  {
    name: 'Display / Impact',
    fonts: [
      { id: 'theboldfont', name: 'THEBOLDFONT' },
      { id: 'bebas-neue', name: 'Bebas Neue' },
      { id: 'anton', name: 'Anton' },
      { id: 'lilita-one', name: 'Lilita One' },
      { id: 'oswald-bold', name: 'Oswald Bold' },
      { id: 'bangers-regular', name: 'Bangers Regular' },
    ],
  },
  {
    name: 'Fun / Comic',
    fonts: [
      { id: 'komika-axis', name: 'Komika Axis' },
      { id: 'comic-neue', name: 'Comic Neue' },
      { id: 'fredoka', name: 'Fredoka' },
      { id: 'chewy', name: 'Chewy' },
      { id: 'luckiest-guy', name: 'Luckiest Guy' },
    ],
  },
  {
    name: 'Serif / Elegant',
    fonts: [
      { id: 'playfair-display', name: 'Playfair Display' },
      { id: 'merriweather', name: 'Merriweather' },
      { id: 'lora', name: 'Lora' },
      { id: 'cinzel', name: 'Cinzel' },
      { id: 'bodoni-moda', name: 'Bodoni Moda' },
    ],
  },
  {
    name: 'Handwritten / Script',
    fonts: [
      { id: 'permanent-marker', name: 'Permanent Marker' },
      { id: 'patrick-hand', name: 'Patrick Hand' },
      { id: 'amatic-sc', name: 'Amatic SC' },
      { id: 'caveat-brush', name: 'Caveat Brush' },
      { id: 'pacifico', name: 'Pacifico' },
    ],
  },
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
  'open-sans': 'Open Sans',
  lato: 'Lato',
  raleway: 'Raleway',
  'bebas-neue': 'Bebas Neue',
  anton: 'Anton',
  'lilita-one': 'Lilita One',
  'comic-neue': 'Comic Neue',
  fredoka: 'Fredoka',
  chewy: 'Chewy',
  'luckiest-guy': 'Luckiest Guy',
  'playfair-display': 'Playfair Display',
  merriweather: 'Merriweather',
  lora: 'Lora',
  cinzel: 'Cinzel',
  'bodoni-moda': 'Bodoni Moda',
  'permanent-marker': 'Permanent Marker',
  'patrick-hand': 'Patrick Hand',
  'amatic-sc': 'Amatic SC',
  'caveat-brush': 'Caveat Brush',
  pacifico: 'Pacifico',
} as const

const getFontName = (fontId: string): string => {
  return FONT_NAMES[fontId as keyof typeof FONT_NAMES] || 'Montserrat Black'
}

export function TemplatePreviewCard({
  template,
  isSelected,
  onSelect,
  previewFrame,
  isBurnedPreview,
  isLoading,
  isGenerating,
}: {
  template: Template
  isSelected: boolean
  onSelect: () => void
  previewFrame?: string | null // Base64 image data
  isBurnedPreview?: boolean
  isLoading?: boolean
  isGenerating?: boolean
}) {
  const [isHovered, setIsHovered] = React.useState(false)

  // Function to render the styled caption preview
  const renderCaptionPreview = () => {
    // Basic style mapping - this is a simplified version of what the backend does
    const baseStyle = {
      fontFamily: template.font,
      color: template.textColor,
      WebkitTextStroke: template.outlineColor ? `2px ${template.outlineColor}` : 'none',
      textShadow: template.glowEffect ? `0 0 10px ${template.highlightWordColor}` : 'none',
    }

    // Different layouts based on position
    const positionClass = cn(
      'absolute left-0 right-0 text-center px-4',
      template.position === 'top' && 'top-12',
      template.position === 'top-quarter' && 'top-1/4',
      template.position === 'center' && 'top-1/2 -translate-y-1/2',
      template.position === 'bottom-quarter' && 'bottom-1/4',
      template.position === 'bottom' && 'bottom-8'
    )

    return (
      <div className={positionClass}>
        <div className="flex flex-col items-center justify-center gap-1">
          <span style={baseStyle} className="text-lg font-bold leading-tight break-words max-w-full">
            LOREM IPSUM
          </span>
          <span
            style={{ ...baseStyle, color: template.highlightWordColor }}
            className="text-lg font-bold leading-tight break-words max-w-full"
          >
            DOLOR SIT AMET
          </span>
        </div>
      </div>
    )
  }

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
          {isLoading ? (
            <div className="relative w-full h-full flex flex-col items-center justify-center bg-black/80 gap-2 p-2">
              <Loader2 className="w-5 h-5 animate-spin text-primary" data-testid="preview-loading-spinner" />
              <span className="text-[10px] text-muted-foreground text-center font-medium">Loading preview…</span>
            </div>
          ) : previewFrame ? (
            <div className="relative w-full h-full">
              <img
                src={previewFrame}
                className="w-full h-full object-cover animate-in fade-in duration-300"
                alt="Preview"
              />
              {/* Caption Overlay - only show if not burned preview */}
              {!isBurnedPreview && renderCaptionPreview()}
              {isGenerating && (
                <div
                  className="absolute top-2 left-2 z-10 p-1 rounded-full bg-black/60 backdrop-blur-xs border border-white/10"
                  title="Generating template preview"
                >
                  <Loader2 className="w-3 h-3 animate-spin text-primary" />
                </div>
              )}
            </div>
          ) : (
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
          )}

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
  whisperServerUrl,
}: {
  onSave: (apiKey: string, whisperServerUrl: string) => void
  isOpen: boolean
  onOpenChange: (open: boolean) => void
  apiKey: string
  whisperServerUrl: string
}) {
  const [apiKeyState, setApiKeyState] = useState(apiKey)
  const [whisperServerUrlState, setWhisperServerUrlState] = useState(whisperServerUrl)

  useEffect(() => {
    setApiKeyState(apiKey)
  }, [apiKey])

  useEffect(() => {
    setWhisperServerUrlState(whisperServerUrl)
  }, [whisperServerUrl])

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

          <div className="grid gap-2">
            <label htmlFor="whisper-server" className="text-sm text-white/40 font-medium">
              Whisper Server URL
            </label>
            <Input
              id="whisper-server"
              placeholder="http://192.168.1.50:8000"
              className="w-full px-4 py-3 border border-border rounded-lg bg-background/50 focus:outline-none hover:border-primary/50 focus:border-primary/70 ring-0 focus-visible:ring-2 focus-visible:ring-primary/20 focus:ring-2 focus:ring-primary/20 text-foreground transition-all duration-200"
              value={whisperServerUrlState}
              onChange={(e) => setWhisperServerUrlState(e.target.value)}
            />
            <p className="text-xs text-white/30 leading-relaxed">
              Optional. Point CapSlap at a whisper.cpp server (or any OpenAI-compatible Whisper API) running on another
              machine, e.g. your Mac Studio:{' '}
              <code className="text-white/50">./server -m ggml-base.bin --port 8000</code>. When set, transcription runs
              on that machine instead of locally.
            </p>
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
                onSave(apiKeyState, whisperServerUrlState.trim())
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

export function FileCard({ path, isLoading, onRemove }: { path: string; isLoading?: boolean; onRemove: () => void }) {
  const fileName = path.split('/').pop() || ''

  return (
    <div className="group relative bg-gradient-to-br from-card/80 to-card/40 border border-border/30 rounded-xl p-4 hover:border-primary/40 transition-all duration-300 backdrop-blur-sm">
      <div className="flex items-center gap-4">
        <div className="relative flex items-center justify-center w-6 h-6">
          {isLoading ? (
            <Loader2 className="w-5 h-5 text-primary animate-spin" data-testid="file-loading-spinner" />
          ) : (
            <FileVideo className="w-6 h-6 text-white" />
          )}
        </div>

        <div className="flex-1 min-w-0">
          <p className="font-medium text-foreground truncate text-sm">{fileName}</p>
          {isLoading && (
            <p className="text-[11px] text-primary/80 flex items-center gap-1 font-medium">Loading preview frame…</p>
          )}
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
  const [isLoadingPreviewFrame, setIsLoadingPreviewFrame] = useState(false)
  const [isGeneratingPreviews, setIsGeneratingPreviews] = useState(false)
  const [isDragOver, setIsDragOver] = useState(false)
  const [_, setDragCounter] = useState(0)
  const [isApiKeySettingsOpen, setIsApiKeySettingsOpen] = useState(false)
  const [shouldGenerateAfterApiKey, setShouldGenerateAfterApiKey] = useState(false)
  const [currentRequestId, setCurrentRequestId] = useState<string | null>(null)
  const [exportProgress, setExportProgress] = useState(0)
  const [exportStatus, setExportStatus] = useState('')
  const [previewFrames, setPreviewFrames] = useState<Record<string, string>>({}) // Map of template ID to base64 image
  const [rawPreviewFrame, setRawPreviewFrame] = useState<string | null>(null) // Fallback raw frame
  const previewCache = React.useRef<Map<string, string>>(new Map())

  // Editor state
  const [isEditorOpen, setIsEditorOpen] = useState(false)
  const [editorSegments, setEditorSegments] = useState<CaptionSegment[]>([])
  const [editorVideoPath, setEditorVideoPath] = useState<string>('')
  const [editorJobId, setEditorJobId] = useState<string>('')
  // Manual caption placements, kept out of the segments so timings stay untouched.
  const [positionOverrides, setPositionOverrides] = useState<PositionOverride[]>([])
  // Platforms to keep captions clear of. Lives here because the export needs it
  // as much as the editor does.
  const [shownPlatforms, setShownPlatforms] = useState<PlatformId[]>(loadPlatformSelection)

  const updateShownPlatforms = (ids: PlatformId[]) => {
    setShownPlatforms(ids)
    savePlatformSelection(ids)
  }

  useEffect(() => {
    // Listen for global progress events
    // We cast window.rust to any because onProgress might not be in the global type definition yet
    const rust = window.rust as any
    if (rust && rust.onProgress) {
      const unsubscribe = rust.onProgress((event: any) => {
        // Filter events for the current export operation
        if (event.id === currentRequestId && event.event === 'progress') {
          setExportProgress(event.progress)
          setExportStatus(event.status)
        }
      })
      return () => unsubscribe()
    }
    return undefined
  }, [currentRequestId])

  const generatePreviews = async (videoPath: string, segments: CaptionSegment[]) => {
    if (!videoPath || !segments || segments.length === 0) return

    setIsGeneratingPreviews(true)
    try {
      // Use the start time of the first segment for the preview
      const firstSegment = segments[0]
      // Use middle of the segment for better context
      const timestampMs = firstSegment.startMs + (firstSegment.endMs - firstSegment.startMs) / 2

      // Generate preview for each template in parallel, using cached results where possible
      const entries = await Promise.all(
        templates.map(async (template) => {
          try {
            const isSelected = template.id === videoSettings.selectedTemplate
            const position = isSelected ? videoSettings.captionPosition : template.position
            const captionStyle = isSelected ? videoSettings.captionStyle : template.captionStyle
            const fontName = getFontName(isSelected ? videoSettings.selectedFont : template.font)
            const textColor = isSelected ? videoSettings.textColor : template.textColor
            const highlightWordColor = isSelected ? videoSettings.highlightWordColor : template.highlightWordColor
            const outlineColor = isSelected ? videoSettings.outlineColor : template.outlineColor
            const fontSize = isSelected ? videoSettings.fontSize : 65
            const glowEffect = isSelected ? videoSettings.glowEffect : template.glowEffect
            const exportFormat =
              videoSettings.exportFormats && videoSettings.exportFormats.length > 0
                ? videoSettings.exportFormats[0]
                : '9:16'
            const cropStrategy = videoSettings.cropStrategy ?? 'fit'

            const cacheKey = `${videoPath}:${Math.round(timestampMs)}:${firstSegment.text}:${template.id}:${fontName}:${textColor}:${highlightWordColor}:${outlineColor}:${fontSize}:${position}:${captionStyle}:${glowEffect}:${exportFormat}:${cropStrategy}:${shownPlatforms.join(',')}:${isSelected ? JSON.stringify(positionOverrides) : ''}`

            if (previewCache.current.has(cacheKey)) {
              return { id: template.id, imageData: previewCache.current.get(cacheKey)! }
            }

            const result = (await (window as any).rust.call('generatePreviewFrame', {
              inputVideo: videoPath,
              timestampMs: timestampMs,
              segments: [firstSegment], // Only pass the first segment for speed
              targetWidth: 1920, // Preview width
              // Rust struct expects camelCase
              fontName,
              textColor,
              highlightWordColor,
              outlineColor,
              fontSize,
              position,
              karaoke: captionStyle === 'karaoke' || captionStyle === 'karaoke-multiline',
              multiline: captionStyle === 'karaoke-multiline',
              glowEffect,
              exportFormat,
              outputSize: '1080p',
              cropStrategy,
              fitMode: 'cover',
              positionOverrides: isSelected ? positionOverrides : [],
              blockedBands: blockedBandsFor(shownPlatforms),
            })) as { imageData: string }

            if (result && result.imageData) {
              // Keep preview cache bounded to 50 items
              if (previewCache.current.size > 50) {
                const firstKey = previewCache.current.keys().next().value
                if (firstKey) previewCache.current.delete(firstKey)
              }
              previewCache.current.set(cacheKey, result.imageData)
              return { id: template.id, imageData: result.imageData }
            }
            return { id: template.id, imageData: null }
          } catch (e) {
            console.error(`Failed to generate preview for ${template.id}`, e)
            return { id: template.id, imageData: null }
          }
        })
      )

      const newPreviewFrames: Record<string, string> = {}
      for (const entry of entries) {
        if (entry.imageData) {
          newPreviewFrames[entry.id] = entry.imageData
        }
      }
      setPreviewFrames(newPreviewFrames)
    } finally {
      setIsGeneratingPreviews(false)
    }
  }

  // Update previews when editor segments or RELEVANT video settings change
  // Pause dashboard template generation while the full-screen editor is open to dedicate all CPU/GPU to the editor
  useEffect(() => {
    if (editorSegments.length > 0 && editorVideoPath && !isEditorOpen) {
      const timer = setTimeout(() => {
        generatePreviews(editorVideoPath, editorSegments)
      }, 500) // Debounce 500ms to avoid rapid regeneration while picking color

      return () => clearTimeout(timer)
    }
    return undefined
  }, [
    isEditorOpen,
    editorSegments,
    editorVideoPath,
    // Add relevant settings dependencies
    videoSettings.selectedFont,
    videoSettings.textColor,
    videoSettings.highlightWordColor,
    videoSettings.outlineColor,
    videoSettings.glowEffect,
    videoSettings.cropStrategy,
    videoSettings.captionPosition,
    videoSettings.captionStyle,
    videoSettings.selectedTemplate,
    videoSettings.exportFormats,
    videoSettings.fontSize,
  ])

  useEffect(() => {
    const savedSettings = localStorage.getItem('settings-v3')
    const savedApiKey = localStorage.getItem('api-key-v1')

    if (savedApiKey) {
      setApiKey(savedApiKey)
    }

    if (savedSettings) {
      try {
        const cachedVideoSettings: Settings = JSON.parse(savedSettings)
        setVideoSettings({ ...defaultSettings, ...cachedVideoSettings })
      } catch (e) {
        console.error('Failed to parse settings', e)
        setVideoSettings(defaultSettings)
      }
    } else {
      setVideoSettings(defaultSettings)
    }
    setIsLoaded(true)
  }, [])

  useEffect(() => {
    if (!isLoaded) return
    localStorage.setItem('settings-v3', JSON.stringify(videoSettings))
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
      fontSize: 65, // Reset to default when switching templates to ensure good baseline
    })
  }

  /** Load the first frame of a newly added video into the preview area. */
  const loadPreviewFrameFor = async (videoPath: string) => {
    setIsLoadingPreviewFrame(true)
    try {
      const result = (await window.rust.call('extractFirstFrame', { videoPath })) as {
        imageData: string
      }
      if (result && result.imageData) {
        setRawPreviewFrame(result.imageData)
      }
    } catch (e) {
      console.error('Failed to extract frame preview', e)
    } finally {
      setIsLoadingPreviewFrame(false)
    }
  }

  const handleOpenRecent = async (path: string) => {
    rememberRecentVideos([path])
    if (!selectedVideos.includes(path)) {
      setSelectedVideos((prev) => [...prev, path])
    }
    await loadPreviewFrameFor(path)
  }

  const handleVideoSelect = async () => {
    try {
      const paths = await window.rust.openFiles?.([
        {
          name: 'Video Files',
          extensions: ['mp4', 'mov', 'mkv', 'avi', 'webm', 'wmv', 'flv', 'mpeg', 'mpg', 'm4v', '3gp', 'ts', '*'],
        },
      ])

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
        rememberRecentVideos(pathsWithoutDuplicates)

        // Extract frame from the first new video
        if (pathsWithoutDuplicates.length > 0) {
          await loadPreviewFrameFor(pathsWithoutDuplicates[0])
        }
      }
    } catch (error) {
      // Silent error handling
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

  const handleSaveApiKey = (newKey: string, whisperServerUrl: string) => {
    const key = newKey.trim()
    setApiKey(key)
    localStorage.setItem('api-key-v1', key)

    if (whisperServerUrl !== videoSettings.whisperServerUrl) {
      updateSettings({ whisperServerUrl })
    }

    // If generation was queued behind the API-key prompt, resume it with the
    // freshly saved values. We pass them explicitly because React state from
    // setApiKey isn't visible inside handleGenerate's current closure.
    if (shouldGenerateAfterApiKey) {
      setShouldGenerateAfterApiKey(false)
      handleGenerate(key, whisperServerUrl.trim() || videoSettings.whisperServerUrl.trim())
    }
  }

  const handleCancel = async () => {
    if (currentRequestId) {
      toast.info('Cancelling generation...')
      await window.rust.call('cancel', currentRequestId)
    }
  }

  const handleBurn = async (segments: CaptionSegment[]) => {
    setIsEditorOpen(false)
    setIsGenerating(true)
    setExportStatus('Burning captions...')

    try {
      // Save adjusted captions first
      await window.rust.call('saveCaptions', {
        videoPath: editorVideoPath,
        segments: segments.map((seg) => ({
          ...seg,
          startMs: Math.round(seg.startMs),
          endMs: Math.round(seg.endMs),
          words: seg.words.map((w) => ({
            ...w,
            startMs: Math.round(w.startMs),
            endMs: Math.round(w.endMs),
          })),
        })),
        positionOverrides,
      })

      await window.rust.call(
        'burn',
        {
          inputVideo: editorVideoPath,
          segments: segments.map((seg) => ({
            ...seg,
            startMs: Math.round(seg.startMs),
            endMs: Math.round(seg.endMs),
            words: seg.words.map((w) => ({
              ...w,
              startMs: Math.round(w.startMs),
              endMs: Math.round(w.endMs),
            })),
          })),
          exportFormats: videoSettings.exportFormats,
          karaoke: videoSettings.captionStyle === 'karaoke' || videoSettings.captionStyle === 'karaoke-multiline',
          multiline: videoSettings.captionStyle === 'karaoke-multiline',
          fontName: getFontName(videoSettings.selectedFont),
          textColor: videoSettings.textColor,
          highlightWordColor: videoSettings.highlightWordColor,
          outlineColor: videoSettings.outlineColor,
          glowEffect: videoSettings.glowEffect,
          position: videoSettings.captionPosition,
          outputSize: videoSettings.outputSize,
          cropStrategy: videoSettings.cropStrategy,
          fontSize: videoSettings.fontSize,
          positionOverrides,
          blockedBands: blockedBandsFor(shownPlatforms),
        },
        editorJobId
      )

      toast.success('Video exported successfully!')
    } catch (error: any) {
      showErrorToast(error)
    } finally {
      setIsGenerating(false)
      setEditorJobId('')
      setEditorVideoPath('')
      setCurrentRequestId(null)
    }
  }

  const handleGenerate = async (apiKeyOverride?: string, whisperServerUrlOverride?: string) => {
    if (!selectedVideos.length) {
      toast.error('Please select a video first')
      return
    }

    // Prepare ID and paths early for checking/loading
    const requestId = crypto.randomUUID()
    const video = selectedVideos[0]

    const activeApiKey = (apiKeyOverride ?? apiKey).trim() || ''
    const activeWhisperServerUrl = (whisperServerUrlOverride ?? videoSettings.whisperServerUrl).trim() || ''

    setIsGenerating(true)
    setExportProgress(0)
    setExportStatus('Checking saved captions...')

    try {
      // Check for existing captions
      const result = (await window.rust.call('loadCaptions', { videoPath: video })) as {
        segments: CaptionSegment[] | null
        positionOverrides?: PositionOverride[]
      }

      if (result && result.segments && result.segments.length > 0) {
        console.log('Loaded segments from disk', result.segments)
        setEditorSegments(result.segments)
        setPositionOverrides(result.positionOverrides ?? [])
        setEditorVideoPath(video)
        setEditorJobId(requestId)
        setIsGenerating(false)
        setIsEditorOpen(true)
        toast.info('Loaded saved captions layout!')
        return
      }
    } catch (e) {
      console.warn('Failed to load existing captions', e)
      // Continue to generation if load fails
    }

    // A remote Whisper server doesn't need an OpenAI API key, so only prompt
    // for the key when using the OpenAI model with no server configured.
    if (!activeApiKey && videoSettings.selectedModel === 'whisper-1' && !activeWhisperServerUrl) {
      setIsGenerating(false)
      setShouldGenerateAfterApiKey(true)
      setIsApiKeySettingsOpen(true)
      return
    }

    try {
      setExportProgress(0)
      setExportStatus('Transcribing...')

      console.log('Starting transcription...')

      setCurrentRequestId(requestId)
      setEditorJobId(requestId)
      setEditorVideoPath(video)

      if (selectedVideos.length > 1) {
        toast.info('Batch mode not supported in editor yet. Processing first video only.')
      }

      const result = (await window.rust.call(
        'transcribe',
        {
          inputVideo: video,
          exportFormats: videoSettings.exportFormats,
          karaoke: videoSettings.captionStyle === 'karaoke' || videoSettings.captionStyle === 'karaoke-multiline',
          multiline: videoSettings.captionStyle === 'karaoke-multiline',
          fontName: getFontName(videoSettings.selectedFont),
          splitByWords: false,
          model: videoSettings.selectedModel,
          language: videoSettings.selectedLanguage,
          prompt: null,
          textColor: videoSettings.textColor,
          highlightWordColor: videoSettings.highlightWordColor,
          outlineColor: videoSettings.outlineColor,
          glowEffect: videoSettings.glowEffect,
          position: videoSettings.captionPosition,
          outputSize: videoSettings.outputSize,
          cropStrategy: videoSettings.cropStrategy,
          apiKey: activeApiKey,
          whisperBaseUrl: activeWhisperServerUrl || undefined,
          fontSize: videoSettings.fontSize,
        },
        requestId
      )) as any

      if (result && result.transcription && result.transcription.segments) {
        setEditorSegments(result.transcription.segments)
        setPositionOverrides([]) // Fresh captions start at the style's default position
        setIsGenerating(false) // Stop spinner, ready for edit
        setIsEditorOpen(true)
      } else {
        throw new Error('No transcription results')
      }
    } catch (error: any) {
      showErrorToast(error, 1)
      setIsGenerating(false)
      setCurrentRequestId(null)
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

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setIsDragOver(false)
    setDragCounter(0)

    const files = Array.from(e.dataTransfer.files)

    if (files.length > 0) {
      const filesPaths = files.map((file) => window.rust.getFilePath(file)).filter((path) => path !== null)

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

        // Extract frame from the first new video (drag and drop)
        const validNewPaths = pathsWithoutDuplicates.filter((path) => path !== null) as string[]
        rememberRecentVideos(validNewPaths)
        if (validNewPaths.length > 0) {
          await loadPreviewFrameFor(validNewPaths[0])
        }
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

            <div className="relative z-10 flex flex-col items-center justify-center p-12 rounded-3xl border-2 border-dashed border-primary/70 bg-gradient-to-br from-primary/20 via-primary/10 to-primary/20 backdrop-blur-xl animate-in zoom-in-95 fade-in duration-300 shadow-2xl">
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
                    <span className="px-1.5 py-0.5 bg-primary/20 rounded text-xs font-mono">All Video Formats</span>
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
                whisperServerUrl={videoSettings.whisperServerUrl}
              />
            </div>
            <p className="text-sm text-muted-foreground truncate">Lightning-fast AI captions</p>
          </div>

          <div className="p-4 border-b border-border/50">
            {isGenerating ? (
              <div className="space-y-3">
                <div className="space-y-1.5">
                  <div className="flex justify-between text-xs text-muted-foreground font-medium">
                    <span className="truncate max-w-[70%]">
                      {(exportStatus || 'Processing...').replace(/\s*\(\d+%\).*$/, '')}
                    </span>
                    <span>{Math.round(exportProgress * 100)}%</span>
                  </div>
                  <div className="h-2 bg-secondary/50 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-primary rounded-full transition-all duration-300 ease-out shadow-[0_0_10px_rgba(var(--primary),0.5)]"
                      style={{ width: `${Math.max(5, exportProgress * 100)}%` }}
                    />
                  </div>
                </div>

                <Button
                  variant="destructive"
                  className="w-full text-white bg-red-500 hover:bg-red-600 shadow-md transition-all duration-200"
                  onClick={handleCancel}
                >
                  Abort Generation
                </Button>
              </div>
            ) : (
              <div className="text-center text-sm text-muted-foreground">Ready to generate</div>
            )}
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
                    previewFrame={previewFrames[template.id] || rawPreviewFrame}
                    isBurnedPreview={!!previewFrames[template.id]}
                    isLoading={isLoadingPreviewFrame}
                    isGenerating={isGeneratingPreviews}
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
                      onValueChange={(
                        value: 'karaoke' | 'karaoke-multiline' | 'oneliner' | 'vibrant' | 'storyteller'
                      ) => updateSettings({ captionStyle: value })}
                    >
                      <SelectTrigger className="w-full h-8 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="karaoke">Karaoke</SelectItem>
                        <SelectItem value="karaoke-multiline">Karaoke (2 Lines)</SelectItem>
                        <SelectItem value="oneliner">Oneliner</SelectItem>
                        <SelectItem value="vibrant">Vibrant</SelectItem>
                        <SelectItem value="storyteller">Storyteller</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div>
                    <label className="text-xs text-muted-foreground mb-1 block">Position</label>
                    <Select
                      value={videoSettings.captionPosition}
                      onValueChange={(value) => updateSettings({ captionPosition: value as any })}
                    >
                      <SelectTrigger className="w-full h-9 bg-background/50 border-border hover:border-primary/50 text-xs">
                        <SelectValue placeholder="Position" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="top">Top</SelectItem>
                        <SelectItem value="top-quarter">1/4 from Top</SelectItem>
                        <SelectItem value="center">Center</SelectItem>
                        <SelectItem value="bottom-quarter">1/4 from Bottom</SelectItem>
                        <SelectItem value="bottom">Bottom</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div>
                    <label className="text-xs text-muted-foreground mb-1 block">Language</label>
                    <Select
                      value={videoSettings.selectedLanguage}
                      onValueChange={(value) => updateSettings({ selectedLanguage: value })}
                    >
                      <SelectTrigger className="w-full h-8 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="auto">Detect language</SelectItem>
                        <SelectItem value="en">English</SelectItem>
                        <SelectItem value="fi">Suomi (Finnish)</SelectItem>
                        <SelectItem value="ar">العربية (Arabic)</SelectItem>
                        <SelectItem value="es">Español (Spanish)</SelectItem>
                        <SelectItem value="fr">Français (French)</SelectItem>
                        <SelectItem value="de">Deutsch (German)</SelectItem>
                        <SelectItem value="ru">Русский (Russian)</SelectItem>
                        <SelectItem value="zh">中文 (Chinese)</SelectItem>
                        <SelectItem value="ja">日本語 (Japanese)</SelectItem>
                        <SelectItem value="ko">한국어 (Korean)</SelectItem>
                        <SelectItem value="pt">Português (Portuguese)</SelectItem>
                        <SelectItem value="it">Italiano (Italian)</SelectItem>
                        <SelectItem value="hi">हिन्दी (Hindi)</SelectItem>
                        <SelectItem value="tr">Türkçe (Turkish)</SelectItem>
                        <SelectItem value="nl">Nederlands (Dutch)</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div>
                    <label className="text-xs text-muted-foreground mb-1 block">Font</label>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button
                          variant="outline"
                          className="w-full justify-between px-3 h-8 text-xs font-normal border-input bg-transparent hover:bg-accent hover:text-accent-foreground"
                        >
                          <span className="truncate">
                            {fontCategories.flatMap((g) => g.fonts).find((f) => f.id === videoSettings.selectedFont)
                              ?.name || videoSettings.selectedFont}
                          </span>
                          <ChevronDown className="h-4 w-4 opacity-50" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent className="w-56" align="start">
                        {fontCategories.map((group) => (
                          <DropdownMenuSub key={group.name}>
                            <DropdownMenuSubTrigger className="text-xs">
                              <span className="mr-2">{group.name}</span>
                            </DropdownMenuSubTrigger>
                            <DropdownMenuSubContent className="width-48">
                              {group.fonts.map((font) => (
                                <DropdownMenuItem
                                  key={font.id}
                                  onSelect={() => updateSettings({ selectedFont: font.id })}
                                  className="text-xs justify-between"
                                >
                                  <span className={cn(font.id === 'komika-axis' ? 'font-komika' : 'font-sans')}>
                                    {font.name}
                                  </span>
                                  {videoSettings.selectedFont === font.id && <Check className="h-3 w-3 ml-2" />}
                                </DropdownMenuItem>
                              ))}
                            </DropdownMenuSubContent>
                          </DropdownMenuSub>
                        ))}
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>

                  <div>
                    <div className="flex items-center justify-between mb-1">
                      <label className="text-xs text-muted-foreground">Font Size</label>
                      <span className="text-xs text-muted-foreground font-mono">{videoSettings.fontSize}px</span>
                    </div>
                    <Slider
                      value={[videoSettings.fontSize]}
                      min={20}
                      max={200}
                      step={1}
                      onValueChange={([value]) => updateSettings({ fontSize: value })}
                      className="py-2"
                    />
                  </div>
                  {/* </div> */}

                  <div className="space-y-3">
                    <label className="text-xs text-muted-foreground mb-1 block">Output Size</label>
                    <Select
                      value={videoSettings.outputSize || 'original'}
                      onValueChange={(value) => updateSettings({ outputSize: value })}
                    >
                      <SelectTrigger className="w-full h-8 text-xs">
                        <SelectValue placeholder="Resolution" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="original">Original (Source)</SelectItem>
                        <SelectItem value="4k">4K (UHD)</SelectItem>
                        <SelectItem value="1080p">1080p (FHD)</SelectItem>
                        <SelectItem value="720p">720p (HD)</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div className="space-y-3">
                    <label className="text-xs text-muted-foreground mb-1 block">Crop Mode</label>
                    <Select
                      value={videoSettings.cropStrategy || 'fit'}
                      onValueChange={(value) => updateSettings({ cropStrategy: value })}
                    >
                      <SelectTrigger className="w-full h-8 text-xs">
                        <SelectValue placeholder="Strategy" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="fit">Fit (Letterbox)</SelectItem>
                        <SelectItem value="fill">Fill (Center Crop)</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

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
                          ? 'border-primary/70 text-primary'
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

            {/* Download Models */}
            <div className="p-4 border-b border-border/50">
              <ModelDownloader
                selectedModel={videoSettings.selectedModel}
                onSelectModel={(model: ModelInfo['name']) => updateSettings({ selectedModel: model })}
                apiKey={apiKey}
                onOpenApiKeySettings={() => setIsApiKeySettingsOpen(true)}
              />
            </div>
          </div>
        </div>
        {/* Upload sectino */}

        {/* min-w-0: without it this flex child refuses to shrink below the
            intrinsic width of its content, and the caption filmstrip is as wide
            as the video is long. */}
        <div className="relative flex-1 flex flex-col min-w-0">
          {isEditorOpen ? (
            <CaptionEditor
              initialSegments={editorSegments}
              onBurn={handleBurn}
              onCancel={() => {
                setIsEditorOpen(false)
                setEditorJobId('')
              }}
              onRefreshPreview={() => generatePreviews(editorVideoPath, editorSegments)}
              videoPath={editorVideoPath}
              settings={videoSettings}
              fontName={getFontName(videoSettings.selectedFont)}
              positionOverrides={positionOverrides}
              onPositionOverridesChange={setPositionOverrides}
              shownPlatforms={shownPlatforms}
              onShownPlatformsChange={updateShownPlatforms}
              previewFrame={previewFrames[videoSettings.selectedTemplate] || rawPreviewFrame}
              isBurnedPreview={!!previewFrames[videoSettings.selectedTemplate]}
              isLoadingPreview={isLoadingPreviewFrame || isGeneratingPreviews}
            />
          ) : (
            <>
              <div className="relative flex-1 overflow-y-auto scrollbar-hide pb-28">
                {selectedVideos.length === 0 && (
                  <div className="h-full px-8 pt-6 pb-6 flex flex-col">
                    <div
                      className={cn(
                        'group w-full max-w-2xl mx-auto relative flex-1 min-h-[220px] flex flex-col items-center justify-center p-12 rounded-2xl border-2 border-dashed transition-all duration-300 cursor-pointer',
                        'hover:border-primary/70 hover:bg-primary/5',
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
                          <span className="px-1.5 py-0.5 bg-muted rounded text-xs font-mono">All Video Formats</span>
                        </div>
                      </div>
                    </div>

                    <RecentVideos onOpen={handleOpenRecent} />
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
                              onClick={() => {
                                setSelectedVideos([])
                                setRawPreviewFrame(null)
                                setPreviewFrames({})
                              }}
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
                            isLoading={index === 0 && isLoadingPreviewFrame}
                            onRemove={() => {
                              setSelectedVideos((prev) => {
                                const next = prev.filter((p) => p !== path)
                                if (next.length === 0) {
                                  setRawPreviewFrame(null)
                                  setPreviewFrames({})
                                }
                                return next
                              })
                            }}
                          />
                        ))}
                      </div>
                    </div>
                  </div>
                )}
              </div>

              <div className="flex items-center justify-center w-full py-6 border-t border-border/50 absolute bottom-0 bg-background/80 backdrop-blur-xs z-10 px-8">
                <Button
                  onClick={() => handleGenerate()}
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
                      Prepare video
                    </>
                  ) : (
                    <>
                      <Zap className="w-5 h-5 mr-2" />
                      Upload videos first
                    </>
                  )}
                </Button>
              </div>
            </>
          )}
        </div>
      </div>
    </>
  )
}
