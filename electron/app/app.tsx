import { useEffect, useState, useRef } from 'react'
import { Button } from '@/app/components/ui/button'
import { toast, Toaster } from 'sonner'
import { Cog, Zap } from 'lucide-react'
import { TitleBar } from './components/TitleBar'
import { Sidebar } from './components/Sidebar'
import { DragOverlay } from './components/DragOverlay'
import { UploadArea } from './components/UploadArea'
import { VideoList } from './components/VideoList'
import { Template, Settings, defaultSettings, getFontName } from './types'
import { templates } from './constants/templates'
import { showErrorToast } from './utils/errors'

type ProgressData = {
  progress: number
  status: string
}

export default function App() {
  const [selectedVideos, setSelectedVideos] = useState<string[]>([])
  const [videoSettings, setVideoSettings] = useState<Settings>(defaultSettings)
  const [apiKey, setApiKey] = useState('')
  const [isLoaded, setIsLoaded] = useState(false)
  const [isGenerating, setIsGenerating] = useState(false)
  const [isDragOver, setIsDragOver] = useState(false)
  const [_dragCounter, setDragCounter] = useState(0)
  const [isApiKeySettingsOpen, setIsApiKeySettingsOpen] = useState(false)
  const [shouldGenerateAfterApiKey, setShouldGenerateAfterApiKey] = useState(false)
  const [progressMap, setProgressMap] = useState<Map<string, ProgressData>>(new Map())
  const requestIdToVideoPathRef = useRef<Map<string, string>>(new Map())

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

    if (!apiKey && videoSettings.selectedModel === 'whisper-1') {
      setShouldGenerateAfterApiKey(true)
      setIsApiKeySettingsOpen(true)
      return
    }

    try {
      setIsGenerating(true)
      // Clear previous progress
      setProgressMap(new Map())
      requestIdToVideoPathRef.current = new Map()

      // Track pending videos that don't have request IDs yet
      const pendingVideos = new Set(selectedVideos)

      // Register progress handler BEFORE starting async operations
      const unsubscribe = window.rust.onProgress((ev: any) => {
        console.log('[APP] Progress event received:', ev)
        if (ev.event === 'progress' && ev.id) {
          let videoPath = requestIdToVideoPathRef.current.get(ev.id)

          // If we don't have a mapping yet, try to match to first pending video
          if (!videoPath && pendingVideos.size > 0) {
            // Get the first pending video (in order)
            const firstPending = Array.from(pendingVideos)[0]
            videoPath = firstPending
            pendingVideos.delete(firstPending)

            // Update the mapping
            requestIdToVideoPathRef.current.set(ev.id, videoPath)
            console.log('[APP] Mapped request ID', ev.id, 'to video:', videoPath)
          }

          // Update progress if we have a video path
          if (videoPath) {
            console.log('[APP] Updating progress for', videoPath, ':', Math.round(ev.progress * 100), '%', ev.status)
            setProgressMap((prev) => {
              const newMap = new Map(prev)
              newMap.set(videoPath!, {
                progress: Math.round(ev.progress * 100),
                status: ev.status || 'Processing...',
              })
              return newMap
            })
          } else {
            console.log('[APP] No video path found for request ID:', ev.id)
          }
        }
      })

      // Initialize progress for all videos
      setProgressMap((prev) => {
        const newMap = new Map(prev)
        selectedVideos.forEach((video) => {
          if (!newMap.has(video)) {
            newMap.set(video, {
              progress: 0,
              status: 'Starting...',
            })
          }
        })
        return newMap
      })

      // Start all requests
      const requestPromises = selectedVideos.map((video) =>
        window.rust.call('generateCaptions', {
          inputVideo: video,
          exportFormats: videoSettings.exportFormats,
          karaoke: videoSettings.captionStyle === 'karaoke',
          fontName: getFontName(videoSettings.selectedFont),
          splitByWords: true,
          model: videoSettings.selectedModel,
          language: videoSettings.selectedLanguage,
          prompt: null,
          textColor: videoSettings.textColor,
          highlightWordColor: videoSettings.highlightWordColor,
          outlineColor: videoSettings.outlineColor,
          glowEffect: videoSettings.glowEffect,
          position: videoSettings.captionPosition,
          apiKey: apiKey,
        })
      )

      // Wait for all requests to complete
      const results = await Promise.allSettled(requestPromises)

      // Clean up progress handler
      unsubscribe()

      // Mark all videos as complete (only after promise resolves)
      setProgressMap((prev) => {
        const newMap = new Map(prev)
        // Update all videos to show 100% with "Complete" status
        selectedVideos.forEach((video) => {
          newMap.set(video, {
            progress: 100,
            status: 'Complete',
          })
        })
        return newMap
      })

      const successful = results.filter((r) => r.status === 'fulfilled')
      const failed = results.filter((r) => r.status === 'rejected')

      if (successful.length > 0) {
        toast.success(`Generated ${successful.length} videos successfully!`)
      }

      if (failed.length > 0) {
        const errors = failed.map((f) => f.reason)
        const uniqueErrors = [...new Set(errors.map((err) => err.name || 'UNKNOWN_ERROR'))]

        if (uniqueErrors.length === 1) {
          const sampleError = errors[0]
          showErrorToast(sampleError, failed.length)
        } else {
          toast.error(`Failed to process ${failed.length} videos`, {
            description: 'Mixed errors occurred. Check settings and try again.',
          })
        }
      }

      // Clear progress after a short delay to show completion
      setTimeout(() => {
        setProgressMap(new Map())
        requestIdToVideoPathRef.current = new Map()
      }, 2000)
    } catch (error: any) {
      showErrorToast(error, 1)
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

        {isDragOver && <DragOverlay />}

        <Sidebar
          templates={templates}
          settings={videoSettings}
          apiKey={apiKey}
          isApiKeySettingsOpen={isApiKeySettingsOpen}
          onSelectTemplate={selectTemplate}
          onUpdateSettings={updateSettings}
          onToggleExportFormat={handleExportFormatToggle}
          onSaveApiKey={handleSaveApiKey}
          onOpenApiKeySettings={() => setIsApiKeySettingsOpen(true)}
          onCloseApiKeySettings={setIsApiKeySettingsOpen}
        />

        <div className="relative flex-1 flex flex-col">
          <div className="relative flex-1 overflow-y-auto scrollbar-hide pb-28">
            {selectedVideos.length === 0 ? (
              <UploadArea isDragOver={isDragOver} onSelect={handleVideoSelect} />
            ) : (
              <VideoList
                videos={selectedVideos}
                progressMap={progressMap}
                onRemove={(path) => setSelectedVideos((prev) => prev.filter((p) => p !== path))}
                onClearAll={() => setSelectedVideos([])}
              />
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
