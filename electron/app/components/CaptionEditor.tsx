import { useMemo, useRef, useState } from 'react'
import { Button } from '@/app/components/ui/button'
import { ArrowLeft, ArrowRight, Pencil, Check, X, Zap, RefreshCw, MoveVertical, Loader2 } from 'lucide-react'
import { Textarea } from '@/app/components/ui/textarea'
import { cn } from '@/lib/utils'
import { CaptionPositionPanel, type PositionOverride } from './CaptionPositionPanel'
import type { PlatformId } from './safe-areas'

// Types (mirrored from app.tsx for now to avoid circular deps)
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
  // selectedModel: ModelInfo['name'] // Skipping types not needed for display
  textColor: string
  highlightWordColor: string
  outlineColor: string
  glowEffect: boolean
  captionStyle: Template['captionStyle']
  captionPosition: 'top' | 'top-quarter' | 'center' | 'bottom-quarter' | 'bottom'
  fontSize: number
  cropStrategy?: string
  // ... other settings
}

export interface WordSpan {
  startMs: number
  endMs: number
  text: string
}

export interface CaptionSegment {
  startMs: number
  endMs: number
  text: string
  words: WordSpan[]
}

interface CaptionEditorProps {
  initialSegments: CaptionSegment[]
  onBurn: (segments: CaptionSegment[]) => void
  onCancel: () => void
  onRefreshPreview?: () => void
  videoPath: string
  settings: Settings
  /** Resolved font family name, e.g. "Montserrat Black". */
  fontName: string
  positionOverrides: PositionOverride[]
  onPositionOverridesChange: (overrides: PositionOverride[]) => void
  shownPlatforms: PlatformId[]
  onShownPlatformsChange: (ids: PlatformId[]) => void
  previewFrame?: string | null
  isBurnedPreview?: boolean
  isLoadingPreview?: boolean
}

export function CaptionEditor({
  initialSegments,
  onBurn,
  onCancel,
  onRefreshPreview,
  videoPath,
  settings,
  fontName,
  positionOverrides,
  onPositionOverridesChange,
  shownPlatforms,
  onShownPlatformsChange,
  previewFrame,
  isLoadingPreview,
}: CaptionEditorProps) {
  const [segments, setSegments] = useState<CaptionSegment[]>(initialSegments)
  // Removed unused video playback state
  const [editingSegmentIndex, setEditingSegmentIndex] = useState<number | null>(null)
  const [editText, setEditText] = useState('')
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [isPositioning, setIsPositioning] = useState(false)
  // Set when a caption is picked in the position view, so the matching text is
  // easy to find in a long list.
  const [highlightedSegmentIndex, setHighlightedSegmentIndex] = useState<number | null>(null)
  // The time range the still in the position view is showing. Captions are drawn
  // a few words at a time, so a sentence in this list is on screen in pieces —
  // this is the piece the user is looking at.
  const [shownRange, setShownRange] = useState<{ startMs: number; endMs: number } | null>(null)
  const segmentRefs = useRef<(HTMLDivElement | null)[]>([])

  const positionStyle = useMemo(
    () => ({
      fontName,
      fontSize: settings.fontSize,
      textColor: settings.textColor,
      highlightWordColor: settings.highlightWordColor,
      outlineColor: settings.outlineColor,
      glowEffect: settings.glowEffect,
      position: settings.captionPosition,
      karaoke: settings.captionStyle === 'karaoke' || settings.captionStyle === 'karaoke-multiline',
      multiline: settings.captionStyle === 'karaoke-multiline',
      exportFormat: settings.exportFormats?.[0] ?? '9:16',
      cropStrategy: settings.cropStrategy,
    }),
    [
      fontName,
      settings.fontSize,
      settings.textColor,
      settings.highlightWordColor,
      settings.outlineColor,
      settings.glowEffect,
      settings.captionPosition,
      settings.captionStyle,
      settings.exportFormats,
      settings.cropStrategy,
    ]
  )

  const handleRefresh = async () => {
    if (onRefreshPreview) {
      setIsRefreshing(true)
      try {
        await onRefreshPreview()
      } finally {
        setIsRefreshing(false)
      }
    }
  }

  /**
   * Which segment does a caption in the position view belong to?
   *
   * Captions are laid out by merging every word and re-splitting to fit the
   * frame, so a caption's bounds rarely match a segment's. The segment sharing
   * the most time with it is the one the words came from.
   */
  const segmentIndexForRange = (range: { startMs: number; endMs: number }): number => {
    let best = -1
    let bestOverlap = 0
    segments.forEach((seg, index) => {
      const overlap = Math.min(seg.endMs, range.endMs) - Math.max(seg.startMs, range.startMs)
      if (overlap > bestOverlap) {
        bestOverlap = overlap
        best = index
      }
    })
    return best
  }

  const revealSegment = (index: number) => {
    if (index < 0) return
    setHighlightedSegmentIndex(index)
    segmentRefs.current[index]?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }

  const handleSelectBlock = (range: { startMs: number; endMs: number }) => {
    setShownRange(range)
    revealSegment(segmentIndexForRange(range))
  }

  /** Is this word part of the caption block the still is showing? */
  const isOnStill = (word: WordSpan) => {
    if (!isPositioning || !shownRange) return false
    const middle = word.startMs + (word.endMs - word.startMs) / 2
    return middle >= shownRange.startMs && middle <= shownRange.endMs
  }

  const handleEditBlock = (range: { startMs: number; endMs: number }) => {
    const index = segmentIndexForRange(range)
    if (index < 0) return
    revealSegment(index)
    setEditingSegmentIndex(index)
    setEditText(segments[index].text)
  }

  // Move the last word of segment[index] to the beginning of segment[index+1]
  const moveLastWordToNext = (index: number) => {
    if (index < 0 || index >= segments.length - 1) return

    const current = segments[index]
    const next = segments[index + 1]

    if (current.words.length === 0) return

    const wordToMove = current.words[current.words.length - 1]

    const newCurrentWords = current.words.slice(0, -1)
    const newNextWords = [wordToMove, ...next.words]

    // Calculate new timings
    // Current segment ends at the new last word's end, or its original start if empty
    const newCurrentEndMs =
      newCurrentWords.length > 0 ? newCurrentWords[newCurrentWords.length - 1].endMs : current.startMs

    // Next segment starts at the moved word's start
    const newNextStartMs = wordToMove.startMs

    const newCurrent: CaptionSegment = {
      ...current,
      words: newCurrentWords,
      text: newCurrentWords.map((w) => w.text).join(' '),
      endMs: newCurrentEndMs,
    }

    const newNext: CaptionSegment = {
      ...next,
      words: newNextWords,
      text: newNextWords.map((w) => w.text).join(' '),
      startMs: newNextStartMs,
    }

    const newSegments = [...segments]
    newSegments[index] = newCurrent
    newSegments[index + 1] = newNext

    setSegments(newSegments)
  }

  // Move the first word of segment[index] to the end of segment[index-1]
  const moveFirstWordToPrev = (index: number) => {
    if (index <= 0 || index >= segments.length) return

    const prev = segments[index - 1]
    const current = segments[index]

    if (current.words.length === 0) return

    const wordToMove = current.words[0]

    const newPrevWords = [...prev.words, wordToMove]
    const newCurrentWords = current.words.slice(1)

    // Prev segment ends at the moved word's end
    const newPrevEndMs = wordToMove.endMs

    // Current segment starts at the new first word's start, or original end if empty
    const newCurrentStartMs = newCurrentWords.length > 0 ? newCurrentWords[0].startMs : current.endMs

    const newPrev: CaptionSegment = {
      ...prev,
      words: newPrevWords,
      text: newPrevWords.map((w) => w.text).join(' '),
      endMs: newPrevEndMs,
    }

    const newCurrent: CaptionSegment = {
      ...current,
      words: newCurrentWords,
      text: newCurrentWords.map((w) => w.text).join(' '),
      startMs: newCurrentStartMs,
    }

    const newSegments = [...segments]
    newSegments[index - 1] = newPrev
    newSegments[index] = newCurrent

    setSegments(newSegments)
  }

  const handleSaveEdit = (index: number) => {
    const seg = segments[index]
    const newText = editText.trim()

    if (!newText) return

    // Split into words
    const newWordsTexts = newText.split(/\s+/)
    const wordCount = newWordsTexts.length

    // Linear interpolation of timestamps
    const duration = seg.endMs - seg.startMs
    const wordDuration = duration / wordCount

    const newWords: WordSpan[] = newWordsTexts.map((word, i) => ({
      text: word,
      startMs: seg.startMs + i * wordDuration,
      endMs: seg.startMs + (i + 1) * wordDuration,
    }))

    const newSegment: CaptionSegment = {
      ...seg,
      text: newText,
      words: newWords,
    }

    const newSegments = [...segments]
    newSegments[index] = newSegment
    setSegments(newSegments)
    setEditingSegmentIndex(null)
  }

  return (
    <div className="flex bg-[#08090a] text-white h-full w-full overflow-hidden">
      {/* Left: caption placement, or a still of how the captions will look */}
      {isPositioning ? (
        <div className="flex-1 flex min-w-0 border-r border-white/10">
          <CaptionPositionPanel
            videoPath={videoPath}
            segments={segments}
            style={positionStyle}
            overrides={positionOverrides}
            onOverridesChange={onPositionOverridesChange}
            onSelectBlock={handleSelectBlock}
            onEditBlock={handleEditBlock}
            shownPlatforms={shownPlatforms}
            onShownPlatformsChange={onShownPlatformsChange}
          />
        </div>
      ) : (
        <div className="flex-1 relative bg-black flex flex-col justify-center items-center border-r border-white/10">
          <div className="relative w-full h-full max-h-full aspect-[9/16] max-w-md mx-auto bg-black">
            {/* Refresh Button Overlay */}
            {onRefreshPreview && (
              <div className="absolute top-4 right-4 z-20">
                <Button
                  variant="outline"
                  size="icon"
                  className="rounded-full bg-black/40 border-white/20 hover:bg-black/60 hover:border-white/40 text-white backdrop-blur-sm shadow-xl"
                  onClick={(e) => {
                    e.stopPropagation()
                    handleRefresh()
                  }}
                  disabled={isRefreshing}
                  title="Refresh Preview"
                >
                  <RefreshCw className={cn('w-4 h-4', isRefreshing && 'animate-spin')} />
                </Button>
              </div>
            )}

            {/* Static Preview Image Layer */}
            {isRefreshing ? (
              <div className="absolute inset-0 z-10 bg-black flex flex-col items-center justify-center">
                {previewFrame && (
                  <img src={previewFrame} className="w-full h-full object-contain opacity-30" alt="Preview" />
                )}
                <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-black/50 backdrop-blur-xs">
                  <Loader2 className="w-8 h-8 animate-spin text-primary" />
                  <p className="text-xs font-medium text-white/90">Updating preview…</p>
                </div>
              </div>
            ) : previewFrame ? (
              <div className="absolute inset-0 z-10 bg-black">
                <img src={previewFrame} className="w-full h-full object-contain" alt="Preview" />
              </div>
            ) : isLoadingPreview ? (
              <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-2">
                <Loader2 className="w-8 h-8 animate-spin text-primary" />
                <p className="text-xs text-white/80 font-medium">Loading preview frame…</p>
              </div>
            ) : (
              <div className="flex items-center justify-center h-full text-muted-foreground">
                <p>No preview available</p>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Right: Editor */}
      <div className="w-[450px] flex flex-col h-full bg-[#08090a] border-l border-white/10">
        <div className="flex items-center justify-between border-b border-white/10 p-4 shrink-0">
          <h2 className="text-xl font-medium">Edit Captions</h2>
          <div className="flex gap-2">
            <Button
              variant={isPositioning ? 'default' : 'outline'}
              size="sm"
              onClick={() => setIsPositioning((on) => !on)}
              title="Drag captions to a different height"
            >
              <MoveVertical className="w-4 h-4 mr-2" />
              Position
              {positionOverrides.length > 0 && (
                <span className="ml-1.5 text-[10px] opacity-70">{positionOverrides.length}</span>
              )}
            </Button>
            <Button variant="ghost" onClick={onCancel} size="sm">
              Cancel
            </Button>
            <Button onClick={() => onBurn(segments)} size="sm" className="bg-primary hover:bg-primary/90">
              <Zap className="w-4 h-4 mr-2" />
              Generate Video
            </Button>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto space-y-4 p-4">
          {isPositioning && (
            <p className="text-[11px] leading-relaxed text-white/40">
              A caption is drawn a few words at a time. The highlighted words are the ones on the still — pick another
              frame below it to see the rest.
            </p>
          )}
          {segments.map((seg, idx) => {
            const isEditing = editingSegmentIndex === idx

            if (isEditing) {
              return (
                <div
                  key={idx}
                  ref={(node) => {
                    segmentRefs.current[idx] = node
                  }}
                  className="bg-card border border-primary rounded-lg p-4 flex flex-col gap-3"
                >
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-xs font-mono text-muted-foreground">
                      Editing {formatTime(seg.startMs)} - {formatTime(seg.endMs)}
                    </span>
                  </div>
                  <Textarea
                    value={editText}
                    onChange={(e) => setEditText(e.target.value)}
                    className="min-h-[80px] text-lg font-medium"
                    autoFocus
                  />
                  <div className="flex justify-end gap-2 mt-2">
                    <Button size="sm" variant="ghost" onClick={() => setEditingSegmentIndex(null)}>
                      <X className="w-4 h-4 mr-1" /> Cancel
                    </Button>
                    <Button size="sm" onClick={() => handleSaveEdit(idx)}>
                      <Check className="w-4 h-4 mr-1" /> Save
                    </Button>
                  </div>
                </div>
              )
            }

            return (
              <div
                key={idx}
                ref={(node) => {
                  segmentRefs.current[idx] = node
                }}
                className={cn(
                  'group bg-card/30 border rounded-lg p-4 flex flex-col gap-3 transition-all cursor-pointer relative hover:border-primary/30',
                  highlightedSegmentIndex === idx ? 'border-primary/60 bg-primary/5' : 'border-border/30'
                )}
                onDoubleClick={(e) => {
                  e.stopPropagation()
                  setEditingSegmentIndex(idx)
                  setEditText(seg.text)
                }}
              >
                <div className="flex items-start justify-between">
                  <span className="text-xs font-mono text-muted-foreground bg-black/20 px-1.5 py-0.5 rounded">
                    {formatTime(seg.startMs)} - {formatTime(seg.endMs)}
                  </span>

                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6 transition-opacity opacity-0 group-hover:opacity-100"
                    onClick={(e) => {
                      e.stopPropagation()
                      setEditingSegmentIndex(idx)
                      setEditText(seg.text)
                    }}
                    title="Edit text"
                  >
                    <Pencil className="w-3 h-3" />
                  </Button>
                </div>

                {/* Words */}
                <div className="flex flex-wrap gap-1.5">
                  {seg.words.map((word, wIdx) => {
                    const onStill = isOnStill(word)
                    return (
                      <span
                        key={wIdx}
                        className={cn(
                          'px-2 py-1 rounded text-sm border transition-colors cursor-default',
                          onStill
                            ? 'bg-primary/25 border-primary/70 text-white'
                            : 'bg-white/5 border-white/10 hover:bg-white/10 text-white/80'
                        )}
                        title={
                          onStill
                            ? `${formatTime(word.startMs)} - ${formatTime(word.endMs)} — on the still`
                            : `${formatTime(word.startMs)} - ${formatTime(word.endMs)}`
                        }
                      >
                        {word.text}
                      </span>
                    )
                  })}
                </div>

                {/* Actions */}
                <div className="flex items-center gap-2 mt-1 transition-opacity opacity-0 group-hover:opacity-100">
                  {/* Move start word to previous */}
                  <Button
                    variant="secondary"
                    size="sm"
                    className="h-6 px-2 text-[10px]"
                    disabled={idx === 0 || seg.words.length === 0}
                    onClick={(e) => {
                      e.stopPropagation()
                      moveFirstWordToPrev(idx)
                    }}
                    title="Move first word to previous segment"
                  >
                    <ArrowLeft className="w-3 h-3 mr-1" />
                    Shift Start
                  </Button>

                  {/* Move last word to next */}
                  <Button
                    variant="secondary"
                    size="sm"
                    className="h-6 px-2 text-[10px]"
                    disabled={idx === segments.length - 1 || seg.words.length === 0}
                    onClick={(e) => {
                      e.stopPropagation()
                      moveLastWordToNext(idx)
                    }}
                    title="Move last word to next segment"
                  >
                    Shift End
                    <ArrowRight className="w-3 h-3 ml-1" />
                  </Button>
                </div>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

function formatTime(ms: number) {
  const s = ms / 1000
  const minutes = Math.floor(s / 60)
  const seconds = Math.floor(s % 60)
  const millis = Math.floor((s % 1) * 100)
  return `${minutes}:${seconds.toString().padStart(2, '0')}.${millis.toString().padStart(2, '0')}`
}
