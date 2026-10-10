import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Button } from '@/app/components/ui/button'
import { Loader2, MoveVertical, RotateCcw, CopyCheck, Wand2, Layers } from 'lucide-react'
import { toast } from 'sonner'
import { cn } from '@/lib/utils'
import type { CaptionSegment } from './CaptionEditor'
import { PLATFORMS, blockedBandsFor, type Platform, type PlatformId } from './safe-areas'

/** A manual vertical placement for the caption block shown during a time range. */
export interface PositionOverride {
  startMs: number
  endMs: number
  /** Caption anchor as a percentage of frame height, measured from the top. */
  yPct: number
}

/** Style inputs the backend needs to lay out and draw captions. */
export interface CaptionStyleParams {
  fontName: string
  fontSize: number
  textColor: string
  highlightWordColor: string
  outlineColor: string
  glowEffect: boolean
  position: string
  karaoke: boolean
  multiline: boolean
  exportFormat: string
  cropStrategy?: string
}

interface PreviewCue {
  startMs: number
  endMs: number
  yPct: number
  groupStartMs: number
  groupEndMs: number
  anchor: 'top' | 'center' | 'bottom'
  lines: { words: { text: string; isHighlighted: boolean }[] }[]
}

/** One draggable caption block: the unit a user moves. */
export interface CaptionBlock {
  startMs: number
  endMs: number
  /** A moment this caption is definitely on screen, for rendering stills. */
  previewMs: number
  text: string
  defaultYPct: number
  anchor: 'top' | 'center' | 'bottom'
}

interface CaptionPositionPanelProps {
  videoPath: string
  segments: CaptionSegment[]
  style: CaptionStyleParams
  overrides: PositionOverride[]
  onOverridesChange: (overrides: PositionOverride[]) => void
  /** A caption was picked, so the text editor can reveal the same one. */
  onSelectBlock?: (range: { startMs: number; endMs: number }) => void
  /** The user asked to edit this caption's words. */
  onEditBlock?: (range: { startMs: number; endMs: number }) => void
  /** Platforms whose interface is drawn over the preview and dodged when rendering. */
  shownPlatforms: PlatformId[]
  onShownPlatformsChange: (ids: PlatformId[]) => void
}

/** Preview renders are FFmpeg jobs; a few at a time keeps the app responsive. */
const MAX_CONCURRENT_RENDERS = 3
/** Height of the rendered filmstrip thumbnails, in pixels. */
const THUMBNAIL_HEIGHT = 200
/** Keep the anchor inside the frame so a caption can never be dragged out of sight. */
const MIN_Y_PCT = 4
const MAX_Y_PCT = 99

interface RenderJob {
  run: () => void
  priority: 'high' | 'low'
  isCancelled: () => boolean
}

let activeRenders = 0
const renderQueue: RenderJob[] = []

function pumpQueue() {
  while (renderQueue.length > 0 && activeRenders < MAX_CONCURRENT_RENDERS) {
    const next = renderQueue.shift()
    if (!next) break
    if (next.isCancelled()) {
      continue
    }
    next.run()
    break
  }
}

/** Run `job` once a render slot frees up, prioritizing high-priority stage previews over filmstrip thumbnails. */
function scheduleRender<T>(
  job: () => Promise<T>,
  priority: 'high' | 'low' = 'low',
  isCancelled: () => boolean = () => false
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const start = () => {
      if (isCancelled()) {
        reject(new Error('Render cancelled'))
        return
      }
      activeRenders++
      job()
        .then(resolve, reject)
        .finally(() => {
          activeRenders--
          pumpQueue()
        })
    }

    if (activeRenders < MAX_CONCURRENT_RENDERS) {
      start()
    } else {
      const renderJob: RenderJob = { run: start, priority, isCancelled }
      if (priority === 'high') {
        const firstLow = renderQueue.findIndex((j) => j.priority === 'low')
        if (firstLow >= 0) {
          renderQueue.splice(firstLow, 0, renderJob)
        } else {
          renderQueue.push(renderJob)
        }
      } else {
        renderQueue.push(renderJob)
      }
    }
  })
}

const blockKey = (block: Pick<CaptionBlock, 'startMs' | 'endMs'>) => `${block.startMs}-${block.endMs}`

const midpointOf = (block: Pick<CaptionBlock, 'startMs' | 'endMs'>) =>
  Math.round(block.startMs + (block.endMs - block.startMs) / 2)

/** The override covering a block, matched the same way the renderer matches it. */
export function findOverride(overrides: PositionOverride[], block: CaptionBlock): PositionOverride | undefined {
  const mid = midpointOf(block)
  return overrides.find((o) => mid >= o.startMs && mid <= o.endMs)
}

export function effectiveYPct(overrides: PositionOverride[], block: CaptionBlock): number {
  return findOverride(overrides, block)?.yPct ?? block.defaultYPct
}

export const clampY = (yPct: number) => Math.min(MAX_Y_PCT, Math.max(MIN_Y_PCT, yPct))

function formatClock(ms: number): string {
  const total = Math.round(ms / 1000)
  return `${Math.floor(total / 60)}:${(total % 60).toString().padStart(2, '0')}`
}

/**
 * Drag captions to a different height over a still of the video.
 *
 * Each block is previewed as two renders: the frame with no captions, and the
 * captions alone on a transparent canvas, both at the block's default position.
 * Moving a caption is then just a CSS shift of the second image over the first,
 * which is exactly what burning it with the new offset will produce — so
 * dragging stays instant no matter how long the video is.
 */
export function CaptionPositionPanel({
  videoPath,
  segments,
  style,
  overrides,
  onOverridesChange,
  onSelectBlock,
  onEditBlock,
  shownPlatforms,
  onShownPlatformsChange,
}: CaptionPositionPanelProps) {
  const [frameSize, setFrameSize] = useState<{ width: number; height: number } | null>(null)
  const [blocks, setBlocks] = useState<CaptionBlock[]>([])
  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const [layoutError, setLayoutError] = useState<string | null>(null)
  // Rendered images, keyed by block. Full-size pairs drive the main stage;
  // thumbnail pairs drive the filmstrip.
  const [stageImages, setStageImages] = useState<Record<string, { frame: string; captions: string }>>({})
  const [thumbImages, setThumbImages] = useState<Record<string, { frame: string; captions: string }>>({})
  const [isDragging, setIsDragging] = useState(false)
  const [stageBox, setStageBox] = useState<{ width: number; height: number } | null>(null)
  const togglePlatform = useCallback(
    (id: PlatformId) => {
      onShownPlatformsChange(
        shownPlatforms.includes(id) ? shownPlatforms.filter((p) => p !== id) : [...shownPlatforms, id]
      )
    },
    [shownPlatforms, onShownPlatformsChange]
  )

  const activePlatforms: Platform[] = useMemo(
    () => PLATFORMS.filter((platform) => shownPlatforms.includes(platform.id)),
    [shownPlatforms]
  )

  const stageRef = useRef<HTMLDivElement>(null)
  const aliveRef = useRef(true)
  const requestedThumbs = useRef(new Set<string>())
  const requestedStages = useRef(new Set<string>())
  const cleanFrameCache = useRef(new Map<string, string>())

  useEffect(() => {
    aliveRef.current = true
    return () => {
      aliveRef.current = false
    }
  }, [])

  // The stage fills the pane and letterboxes the frame inside it, so it never
  // overflows whatever shape the video happens to be. That means the picture is
  // usually smaller than the element, and every offset below has to be measured
  // against the picture rather than the box around it.
  useEffect(() => {
    const node = stageRef.current
    if (!node) return
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect
      if (rect) setStageBox({ width: rect.width, height: rect.height })
    })
    observer.observe(node)
    return () => observer.disconnect()
  }, [])

  // Re-render everything when the look of the captions changes. The key also
  // keeps the callbacks below stable across renders that changed nothing.
  const styleKey = JSON.stringify(style)
  // Identity-stable copy: renders are expensive, so they should restart when
  // the style's *contents* change, not whenever the parent hands us a new object.
  const activeStyle = useMemo(() => JSON.parse(styleKey) as CaptionStyleParams, [styleKey])

  // Fingerprint of what the captions actually say. Editing a caption's words
  // leaves its timings alone, so without the text in the cache key the edited
  // caption keeps showing the render made before the edit.
  const segmentsKey = useMemo(
    () => JSON.stringify(segments.map((segment) => [segment.startMs, segment.endMs, segment.text])),
    [segments]
  )

  // Cached renders are tagged with everything that produced them, so a restyle
  // or an edit simply misses the cache instead of needing an invalidation pass.
  const cachePrefix = `${styleKey}|${segmentsKey}|${shownPlatforms.join(',')}|${videoPath}|`
  const cacheKey = useCallback((key: string) => `${cachePrefix}${key}`, [cachePrefix])

  /** Store a render, dropping anything left over from an earlier version. */
  const rememberImage = useCallback(
    (setImages: typeof setStageImages, key: string, value: { frame: string; captions: string }) => {
      const prefix = cachePrefix
      setImages((prev) => {
        const kept = Object.fromEntries(Object.entries(prev).filter(([k]) => k.startsWith(prefix)))
        kept[key] = value
        return kept
      })
    },
    [cachePrefix]
  )

  const renderFrame = useCallback(
    async (
      timestampMs: number,
      mode: 'clean' | 'captions',
      thumbnailHeight?: number,
      priority: 'high' | 'low' = 'low'
    ): Promise<string | null> => {
      if (mode === 'clean') {
        const cleanKey = `${videoPath}|${timestampMs}|${thumbnailHeight ?? 'full'}`
        const cached = cleanFrameCache.current.get(cleanKey)
        if (cached) return cached
      }

      const result = (await scheduleRender(
        () =>
          window.rust.call('generatePreviewFrame', {
            inputVideo: videoPath,
            segments,
            timestampMs,
            renderMode: mode,
            thumbnailHeight,
            // Captions are drawn where the style puts them; the editor applies
            // the user's offset on top as a plain CSS shift.
            positionOverrides: [],
            blockedBands: blockedBandsFor(shownPlatforms),
            outputSize: '1080p',
            fitMode: 'cover',
            ...activeStyle,
          }),
        priority
      )) as { imageData?: string } | null

      const imageData = result?.imageData ?? null
      if (imageData && mode === 'clean') {
        const cleanKey = `${videoPath}|${timestampMs}|${thumbnailHeight ?? 'full'}`
        cleanFrameCache.current.set(cleanKey, imageData)
      }
      return imageData
    },
    [videoPath, segments, activeStyle, shownPlatforms]
  )

  // Bootstrap: one clean frame tells us the real frame size, which the layout
  // pass needs before it can say where lines break and where captions sit.
  useEffect(() => {
    if (!videoPath || segments.length === 0) return
    let cancelled = false

    const bootstrap = async () => {
      try {
        const first = segments[0]
        const image = await renderFrame(
          midpointOf({ startMs: first.startMs, endMs: first.endMs }),
          'clean',
          undefined,
          'high'
        )
        if (cancelled || !image) return

        const size = await new Promise<{ width: number; height: number }>((resolve, reject) => {
          const probe = new Image()
          probe.onload = () => resolve({ width: probe.naturalWidth, height: probe.naturalHeight })
          probe.onerror = () => reject(new Error('Could not read the preview frame'))
          probe.src = image
        })
        if (cancelled) return
        setFrameSize(size)

        const layout = (await window.rust.call('previewLayout', {
          segments,
          width: size.width,
          height: size.height,
          fontName: activeStyle.fontName,
          fontSize: activeStyle.fontSize,
          textColor: activeStyle.textColor,
          highlightWordColor: activeStyle.highlightWordColor,
          outlineColor: activeStyle.outlineColor,
          position: activeStyle.position,
          karaoke: activeStyle.karaoke,
          multiline: activeStyle.multiline,
          glowEffect: activeStyle.glowEffect,
          positionOverrides: [],
          blockedBands: blockedBandsFor(shownPlatforms),
        })) as { cues?: PreviewCue[] } | null

        if (cancelled) return

        // One entry per on-screen block — the unit the burner actually draws.
        // A sentence is broken into as many blocks as it takes to fit the frame,
        // which in karaoke is a word or two each, so a strip that followed the
        // text list beside this panel would show one still per sentence and hide
        // everything else the video puts on screen. Karaoke also emits a cue per
        // highlighted word on top of that; those share a group and collapse into
        // the one block a user drags.
        const byGroup = new Map<string, CaptionBlock>()
        for (const cue of layout?.cues ?? []) {
          const key = `${cue.groupStartMs}-${cue.groupEndMs}`
          if (byGroup.has(key)) continue
          byGroup.set(key, {
            startMs: cue.groupStartMs,
            endMs: cue.groupEndMs,
            previewMs: Math.round(cue.groupStartMs + (cue.groupEndMs - cue.groupStartMs) / 2),
            text: cue.lines
              .map((line) => line.words.map((word) => word.text).join(' '))
              .join(' ')
              .trim(),
            defaultYPct: cue.yPct,
            anchor: cue.anchor,
          })
        }
        const found: CaptionBlock[] = [...byGroup.values()].sort((a, b) => a.startMs - b.startMs)
        setBlocks(found)
        setSelectedKey((current) => current ?? (found[0] ? blockKey(found[0]) : null))
        setLayoutError(found.length === 0 ? 'No captions to position.' : null)
      } catch (error: any) {
        if (!cancelled) setLayoutError(error?.message || 'Could not load the caption layout.')
      }
    }

    bootstrap()
    return () => {
      cancelled = true
    }
  }, [videoPath, segments, activeStyle, shownPlatforms, renderFrame])

  const selectedBlock = useMemo(
    () => blocks.find((b) => blockKey(b) === selectedKey) ?? blocks[0] ?? null,
    [blocks, selectedKey]
  )

  // Tell the text editor which words are on the still, so it can mark them in
  // the sentence they came from. Held in a ref because the callback is written
  // inline by the parent: depending on it directly would fire on every render.
  const onSelectBlockRef = useRef(onSelectBlock)
  useEffect(() => {
    onSelectBlockRef.current = onSelectBlock
  })
  useEffect(() => {
    if (!selectedBlock) return
    onSelectBlockRef.current?.({ startMs: selectedBlock.startMs, endMs: selectedBlock.endMs })
  }, [selectedBlock])

  // Full-size pair for the block being edited (high priority).
  useEffect(() => {
    if (!selectedBlock) return
    const key = cacheKey(blockKey(selectedBlock))
    if (requestedStages.current.has(key)) return
    requestedStages.current.add(key)

    const load = async () => {
      try {
        const at = selectedBlock.previewMs
        const [frame, captions] = await Promise.all([
          renderFrame(at, 'clean', undefined, 'high'),
          renderFrame(at, 'captions', undefined, 'high'),
        ])
        if (!aliveRef.current || !frame || !captions) return
        rememberImage(setStageImages, key, { frame, captions })
      } catch {
        requestedStages.current.delete(key)
      }
    }
    load()
  }, [selectedBlock, renderFrame, cacheKey, rememberImage])

  const requestThumbnail = useCallback(
    (block: CaptionBlock) => {
      const key = cacheKey(blockKey(block))
      if (requestedThumbs.current.has(key)) return
      requestedThumbs.current.add(key)

      const load = async () => {
        try {
          const at = block.previewMs
          const [frame, captions] = await Promise.all([
            renderFrame(at, 'clean', THUMBNAIL_HEIGHT, 'low'),
            renderFrame(at, 'captions', THUMBNAIL_HEIGHT, 'low'),
          ])
          if (!aliveRef.current || !frame || !captions) return
          rememberImage(setThumbImages, key, { frame, captions })
        } catch {
          requestedThumbs.current.delete(key)
        }
      }
      load()
    },
    [renderFrame, cacheKey, rememberImage]
  )

  // Drop this block's placement while leaving its neighbours where they are.
  //
  // One override can cover several blocks: "Apply to all" writes one per block,
  // but a sidecar saved before the strip followed the on-screen blocks — or one
  // written for a sentence that has since been re-split — spans all of them. So
  // a range that is not this block's own is first pinned onto the other blocks
  // it holds, and only then let go of here.
  const withoutBlock = useCallback(
    (block: CaptionBlock): PositionOverride[] => {
      const covering = findOverride(overrides, block)
      if (!covering) return [...overrides]
      const isOwn = covering.startMs === block.startMs && covering.endMs === block.endMs
      const rest = overrides.filter((o) => o !== covering)
      if (isOwn) return rest
      for (const other of blocks) {
        if (other === block) continue
        if (findOverride([covering], other)) {
          rest.push({ startMs: other.startMs, endMs: other.endMs, yPct: covering.yPct })
        }
      }
      return rest
    },
    [overrides, blocks]
  )

  const setBlockY = useCallback(
    (block: CaptionBlock, yPct: number) => {
      const next = withoutBlock(block)
      next.push({ startMs: block.startMs, endMs: block.endMs, yPct: clampY(yPct) })
      next.sort((a, b) => a.startMs - b.startMs)
      onOverridesChange(next)
    },
    [withoutBlock, onOverridesChange]
  )

  const resetBlock = useCallback(
    (block: CaptionBlock) => {
      onOverridesChange(withoutBlock(block).sort((a, b) => a.startMs - b.startMs))
    },
    [withoutBlock, onOverridesChange]
  )

  const [isAutoPlacing, setIsAutoPlacing] = useState(false)

  const autoPlace = useCallback(async () => {
    setIsAutoPlacing(true)
    try {
      // Whatever overlays are switched on are treated as off limits, so the
      // placement that looks best on the frame is not one the app covers up.
      const blocked = blockedBandsFor(shownPlatforms)

      const result = (await window.rust.call('autoPlaceCaptions', {
        inputVideo: videoPath,
        segments,
        outputSize: '1080p',
        blockedBands: blocked,
        ...activeStyle,
      })) as { positionOverrides?: PositionOverride[]; moved?: number; total?: number } | null

      if (!result) throw new Error('No result')
      onOverridesChange(result.positionOverrides ?? [])

      const moved = result.moved ?? 0
      const avoiding =
        activePlatforms.length > 0 ? ` while avoiding ${activePlatforms.map((p) => p.name).join(' and ')}` : ''
      toast.success(
        moved === 0
          ? `Every caption already sits clear of the picture${avoiding}`
          : `Moved ${moved} of ${result.total ?? 0} captions${avoiding}`,
        { description: 'Drag any of them if you disagree.' }
      )
    } catch (error: unknown) {
      toast.error('Could not place captions automatically', {
        description: error instanceof Error ? error.message : undefined,
      })
    } finally {
      setIsAutoPlacing(false)
    }
  }, [videoPath, segments, activeStyle, shownPlatforms, activePlatforms, onOverridesChange])

  const applyToAll = useCallback(() => {
    if (!selectedBlock) return
    const yPct = effectiveYPct(overrides, selectedBlock)
    onOverridesChange(blocks.map((block) => ({ startMs: block.startMs, endMs: block.endMs, yPct })))
  }, [blocks, overrides, selectedBlock, onOverridesChange])

  // Size and offset of the picture inside the letterboxed stage.
  const picture = useMemo(() => {
    if (!frameSize || !stageBox || stageBox.width === 0 || stageBox.height === 0) {
      return { height: 0, top: 0, width: 0, left: 0 }
    }
    const scale = Math.min(stageBox.width / frameSize.width, stageBox.height / frameSize.height)
    const height = frameSize.height * scale
    const width = frameSize.width * scale
    return {
      height,
      width,
      top: (stageBox.height - height) / 2,
      left: (stageBox.width - width) / 2,
    }
  }, [frameSize, stageBox])

  // Dragging: convert pointer travel into a share of the frame height.
  const dragState = useRef<{ startClientY: number; startYPct: number; height: number } | null>(null)

  const onPointerDown = (event: React.PointerEvent) => {
    if (!selectedBlock || picture.height === 0) return
    dragState.current = {
      startClientY: event.clientY,
      startYPct: effectiveYPct(overrides, selectedBlock),
      height: picture.height,
    }
    setIsDragging(true)
    // Capture on the stage itself so a drag keeps tracking past the frame edge.
    event.currentTarget.setPointerCapture?.(event.pointerId)
  }

  const onPointerMove = (event: React.PointerEvent) => {
    const drag = dragState.current
    if (!drag || !selectedBlock) return
    const deltaPct = ((event.clientY - drag.startClientY) / drag.height) * 100
    setBlockY(selectedBlock, drag.startYPct + deltaPct)
  }

  const endDrag = (event: React.PointerEvent) => {
    if (!dragState.current) return
    dragState.current = null
    setIsDragging(false)
    event.currentTarget.releasePointerCapture?.(event.pointerId)
  }

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (!selectedBlock) return
    const step = event.shiftKey ? 0.2 : 1
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      setBlockY(selectedBlock, effectiveYPct(overrides, selectedBlock) - step)
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      setBlockY(selectedBlock, effectiveYPct(overrides, selectedBlock) + step)
    }
  }

  const aspectRatio = frameSize ? `${frameSize.width} / ${frameSize.height}` : '9 / 16'
  const stage = selectedBlock ? stageImages[cacheKey(blockKey(selectedBlock))] : undefined
  const selectedY = selectedBlock ? effectiveYPct(overrides, selectedBlock) : 0
  const selectedShiftPct = selectedBlock ? selectedY - selectedBlock.defaultYPct : 0
  const isMoved = selectedBlock ? !!findOverride(overrides, selectedBlock) : false
  // In pixels, because the caption layer is letterboxed with the frame and a
  // percentage would be measured against the taller stage box.
  const selectedShiftPx = (selectedShiftPct / 100) * picture.height

  return (
    <div className="flex-1 flex flex-col min-w-0 bg-black">
      {/* Stage */}
      <div className="flex-1 min-h-0 flex items-center justify-center p-4 overflow-hidden">
        <div
          ref={stageRef}
          className={cn(
            'relative w-full h-full bg-black overflow-hidden rounded-md select-none touch-none outline-none',
            selectedBlock && (isDragging ? 'cursor-grabbing' : 'cursor-grab')
          )}
          tabIndex={0}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
          onKeyDown={onKeyDown}
        >
          {stage ? (
            <>
              <img
                src={stage.frame}
                alt="Video frame"
                className="absolute inset-0 w-full h-full object-contain"
                draggable={false}
              />
              <img
                src={stage.captions}
                alt="Captions"
                className="absolute inset-0 w-full h-full object-contain pointer-events-none"
                style={{ transform: `translateY(${selectedShiftPx}px)` }}
                draggable={false}
              />
            </>
          ) : (
            <div className="absolute inset-0 flex items-center justify-center text-white/40 text-sm gap-2">
              {layoutError ? (
                <span className="px-6 text-center">{layoutError}</span>
              ) : (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  Rendering preview…
                </>
              )}
            </div>
          )}

          <SafeAreaOverlay platforms={activePlatforms} picture={picture} />

          {/* Anchor guide, shown while dragging so the target line is visible. */}
          {stage && isDragging && (
            <div
              className="absolute left-0 right-0 border-t border-dashed border-primary/80 pointer-events-none"
              style={{ top: `${picture.top + (selectedY / 100) * picture.height}px` }}
            >
              <span className="absolute right-1 -top-6 text-[10px] font-mono bg-primary text-primary-foreground px-1.5 py-0.5 rounded">
                {selectedY.toFixed(1)}%
              </span>
            </div>
          )}
        </div>
      </div>

      {/* Platform overlays */}
      <div className="flex items-center gap-2 px-4 pt-2 text-[11px] text-white/50 shrink-0">
        <Layers className="w-3.5 h-3.5 shrink-0" />
        <span className="shrink-0">Covered by</span>
        {PLATFORMS.map((platform) => {
          const isOn = shownPlatforms.includes(platform.id)
          return (
            <button
              key={platform.id}
              onClick={() => togglePlatform(platform.id)}
              className={cn(
                'rounded-full border px-2 py-0.5 transition-colors',
                isOn ? 'text-black' : 'border-white/20 text-white/60 hover:border-white/40'
              )}
              style={isOn ? { backgroundColor: platform.color, borderColor: platform.color } : undefined}
              title={`Show what ${platform.name} draws on top of the video`}
            >
              {platform.name}
            </button>
          )
        })}
        {activePlatforms.length > 0 && (
          <span className="truncate text-white/35">approximate, and it shifts between app versions</span>
        )}
      </div>

      {/* Controls */}
      <div className="flex items-center gap-2 px-4 py-2 border-t border-white/10 text-xs text-white/60 shrink-0">
        <MoveVertical className="w-3.5 h-3.5 shrink-0" />
        <span className="truncate">
          {selectedBlock
            ? 'Drag the frame — or use ↑/↓ — to move this caption. Double-click a frame below to edit its words.'
            : 'No caption selected.'}
        </span>
        <div className="ml-auto flex items-center gap-1 shrink-0">
          <Button
            variant="secondary"
            size="sm"
            className="h-7 text-[11px]"
            disabled={isAutoPlacing || blocks.length === 0}
            onClick={autoPlace}
            title="Move every caption to the calmest part of the picture"
          >
            {isAutoPlacing ? <Loader2 className="w-3 h-3 mr-1 animate-spin" /> : <Wand2 className="w-3 h-3 mr-1" />}
            Auto-place
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-[11px]"
            disabled={!selectedBlock || blocks.length < 2}
            onClick={applyToAll}
            title="Give every caption this height"
          >
            <CopyCheck className="w-3 h-3 mr-1" />
            Apply to all
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-[11px]"
            disabled={!isMoved}
            onClick={() => selectedBlock && resetBlock(selectedBlock)}
          >
            <RotateCcw className="w-3 h-3 mr-1" />
            Reset
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-[11px]"
            disabled={overrides.length === 0}
            onClick={() => onOverridesChange([])}
          >
            Reset all
          </Button>
        </div>
      </div>

      {/* Filmstrip */}
      <div className="h-[188px] shrink-0 border-t border-white/10 bg-[#08090a] overflow-x-auto overflow-y-hidden">
        <div className="flex gap-2 p-3 h-full">
          {blocks.map((block) => {
            const key = blockKey(block)
            const thumb = thumbImages[cacheKey(key)]
            const shiftPct = effectiveYPct(overrides, block) - block.defaultYPct
            const moved = !!findOverride(overrides, block)

            return (
              <FilmstripFrame
                key={key}
                block={block}
                isSelected={key === selectedKey}
                thumb={thumb}
                shiftPct={shiftPct}
                moved={moved}
                aspectRatio={aspectRatio}
                onSelect={() => setSelectedKey(key)}
                onEdit={() => onEditBlock?.({ startMs: block.startMs, endMs: block.endMs })}
                onVisible={requestThumbnail}
              />
            )
          })}
        </div>
      </div>
    </div>
  )
}

/**
 * Draws where each selected platform's own interface sits.
 *
 * Positioned against the picture rather than the stage, so the boxes line up
 * with the video no matter how much letterboxing there is around it.
 */
function SafeAreaOverlay({
  platforms,
  picture,
}: {
  platforms: Platform[]
  picture: { top: number; left: number; width: number; height: number }
}) {
  if (platforms.length === 0 || picture.height === 0) return null

  return (
    <div
      className="absolute pointer-events-none"
      style={{
        top: picture.top,
        left: picture.left,
        width: picture.width,
        height: picture.height,
      }}
    >
      {platforms.flatMap((platform) =>
        platform.regions.map((region) => (
          <div
            key={`${platform.id}-${region.label}`}
            className="absolute"
            style={{
              top: `${region.top}%`,
              left: `${region.left}%`,
              width: `${region.width}%`,
              height: `${region.height}%`,
              border: `2px dashed ${platform.color}`,
              backgroundColor: `${platform.color}33`,
              // Dark halo either side of the border so the dashes stay visible
              // over bright footage as well as dark.
              boxShadow: `0 0 0 1px rgba(0,0,0,0.75), inset 0 0 0 1px rgba(0,0,0,0.75)`,
            }}
          >
            <span
              className="absolute top-0 left-0 px-1 py-px text-[9px] font-bold uppercase tracking-wide text-black"
              style={{ backgroundColor: platform.color }}
            >
              {region.label}
            </span>
          </div>
        ))
      )}
    </div>
  )
}

function FilmstripFrame({
  block,
  isSelected,
  thumb,
  shiftPct,
  moved,
  aspectRatio,
  onSelect,
  onEdit,
  onVisible,
}: {
  block: CaptionBlock
  isSelected: boolean
  thumb?: { frame: string; captions: string }
  shiftPct: number
  moved: boolean
  aspectRatio: string
  onSelect: () => void
  onEdit: () => void
  onVisible: (block: CaptionBlock) => void
}) {
  const ref = useRef<HTMLButtonElement>(null)

  // Render thumbnails only for the part of the strip the user has scrolled to;
  // a long video can otherwise mean hundreds of FFmpeg jobs nobody looks at.
  // Re-runs when onVisible changes identity, which is how a restyle gets the
  // visible thumbnails redrawn.
  useEffect(() => {
    const node = ref.current
    if (!node) return
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          onVisible(block)
          observer.disconnect()
        }
      },
      { root: node.parentElement?.parentElement ?? null, rootMargin: '200px' }
    )
    observer.observe(node)
    return () => observer.disconnect()
  }, [onVisible, block])

  return (
    <button
      ref={ref}
      onClick={onSelect}
      onDoubleClick={(event) => {
        event.stopPropagation()
        onEdit()
      }}
      title={`${block.text}\n(double-click to edit the words)`}
      className={cn(
        'group relative h-full shrink-0 rounded overflow-hidden border-2 transition-colors bg-black',
        isSelected ? 'border-primary' : 'border-transparent hover:border-white/30'
      )}
      style={{ aspectRatio }}
    >
      {thumb ? (
        <>
          <img src={thumb.frame} alt="" className="absolute inset-0 w-full h-full object-cover" draggable={false} />
          <img
            src={thumb.captions}
            alt=""
            className="absolute inset-0 w-full h-full object-cover"
            style={{ transform: `translateY(${shiftPct}%)` }}
            draggable={false}
          />
        </>
      ) : (
        <div className="absolute inset-0 flex items-center justify-center bg-white/5 animate-pulse">
          <Loader2 className="w-3.5 h-3.5 animate-spin text-white/30" />
        </div>
      )}

      <span className="absolute bottom-0 inset-x-0 bg-black/70 text-[9px] font-mono text-white/70 px-1 py-0.5">
        {formatClock(block.startMs)}
      </span>
      {moved && (
        <span
          className="absolute top-1 right-1 w-2 h-2 rounded-full bg-primary shadow"
          title="Moved from the default position"
        />
      )}
    </button>
  )
}
