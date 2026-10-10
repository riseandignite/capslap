import type React from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import {
  CaptionPositionPanel,
  clampY,
  effectiveYPct,
  findOverride,
  type CaptionBlock,
  type CaptionStyleParams,
  type PositionOverride,
} from './CaptionPositionPanel'
import type { PlatformId } from './safe-areas'

const block = (startMs: number, endMs: number, defaultYPct = 88): CaptionBlock => ({
  startMs,
  endMs,
  previewMs: Math.round(startMs + (endMs - startMs) / 2),
  defaultYPct,
  text: 'HELLO',
  anchor: 'bottom',
})

describe('override matching', () => {
  it('claims a block whose midpoint falls inside the range', () => {
    const overrides: PositionOverride[] = [{ startMs: 1000, endMs: 2000, yPct: 30 }]
    expect(findOverride(overrides, block(1000, 2000))).toBeDefined()
    expect(effectiveYPct(overrides, block(1000, 2000))).toBe(30)
  })

  it('leaves blocks outside the range at their default', () => {
    const overrides: PositionOverride[] = [{ startMs: 1000, endMs: 2000, yPct: 30 }]
    expect(findOverride(overrides, block(2200, 3000))).toBeUndefined()
    expect(effectiveYPct(overrides, block(2200, 3000))).toBe(88)
  })

  it('still matches after an edit nudges the block boundaries', () => {
    // Shifting the block by 100ms keeps its midpoint inside the saved range,
    // which is why overrides are matched by midpoint rather than by identity.
    const overrides: PositionOverride[] = [{ startMs: 1000, endMs: 2000, yPct: 30 }]
    expect(effectiveYPct(overrides, block(1100, 2100))).toBe(30)
  })

  it('keeps the anchor on screen', () => {
    expect(clampY(-40)).toBe(4)
    expect(clampY(140)).toBe(99)
    expect(clampY(52)).toBe(52)
  })
})

const style: CaptionStyleParams = {
  fontName: 'Montserrat Black',
  fontSize: 65,
  textColor: '#ffffff',
  highlightWordColor: '#ffff00',
  outlineColor: '#000000',
  glowEffect: false,
  position: 'bottom',
  karaoke: false,
  multiline: false,
  exportFormat: '9:16',
}

const segments = [
  {
    startMs: 0,
    endMs: 2000,
    text: 'HELLO THERE',
    words: [
      { startMs: 0, endMs: 1000, text: 'HELLO' },
      { startMs: 1000, endMs: 2000, text: 'THERE' },
    ],
  },
]

/** A 1x1 transparent PNG — enough for happy-dom to report a natural size. */
const PIXEL =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=='

function mockRust(cues: unknown[]) {
  const call = vi.fn(async (method: string, params?: Record<string, unknown>) => {
    void params
    if (method === 'generatePreviewFrame') return { imageData: PIXEL }
    if (method === 'previewLayout') return { cues, fontSizePx: 115 }
    return null
  })
  ;(window as unknown as { rust: { call: typeof call } }).rust = { call }
  return call
}

/** Reports a fixed stage size, since happy-dom does no layout. */
const STAGE_BOX = { width: 600, height: 1000 }

class FixedSizeResizeObserver {
  constructor(private callback: ResizeObserverCallback) {}
  observe(target: Element) {
    this.callback(
      [{ target, contentRect: STAGE_BOX as DOMRectReadOnly } as ResizeObserverEntry],
      this as unknown as ResizeObserver
    )
  }
  unobserve() {}
  disconnect() {}
}

/** Reports every observed element as visible, so lazy thumbnails load. */
class EagerIntersectionObserver {
  constructor(private callback: IntersectionObserverCallback) {}
  observe(target: Element) {
    this.callback(
      [{ isIntersecting: true, target } as IntersectionObserverEntry],
      this as unknown as IntersectionObserver
    )
  }
  unobserve() {}
  disconnect() {}
  takeRecords(): IntersectionObserverEntry[] {
    return []
  }
}

describe('CaptionPositionPanel', () => {
  let platforms: PlatformId[] = []

  beforeEach(() => {
    platforms = []
    global.IntersectionObserver = EagerIntersectionObserver as unknown as typeof IntersectionObserver
    global.ResizeObserver = FixedSizeResizeObserver as unknown as typeof ResizeObserver
    // happy-dom does not decode images; report a size so the layout pass runs.
    Object.defineProperty(window.Image.prototype, 'naturalWidth', { configurable: true, value: 1080 })
    Object.defineProperty(window.Image.prototype, 'naturalHeight', { configurable: true, value: 1920 })
    Object.defineProperty(window.Image.prototype, 'src', {
      configurable: true,
      set() {
        setTimeout(() => this.onload?.(new Event('load')), 0)
      },
    })
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('collapses karaoke word cues into one draggable block per line', async () => {
    // Two word-level cues that belong to the same on-screen line.
    const call = mockRust([
      {
        startMs: 0,
        endMs: 1000,
        yPct: 88,
        groupStartMs: 0,
        groupEndMs: 2000,
        anchor: 'bottom',
        lines: [{ words: [{ text: 'HELLO', isHighlighted: true }] }],
      },
      {
        startMs: 1000,
        endMs: 2000,
        yPct: 88,
        groupStartMs: 0,
        groupEndMs: 2000,
        anchor: 'bottom',
        lines: [{ words: [{ text: 'THERE', isHighlighted: true }] }],
      },
    ])

    await act(async () => {
      render(
        <CaptionPositionPanel
          videoPath="/tmp/clip.mp4"
          segments={segments}
          style={style}
          overrides={[]}
          onOverridesChange={vi.fn()}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      )
    })

    // One filmstrip frame, not one per karaoke word.
    await waitFor(() => {
      expect(screen.getAllByTitle(/^HELLO/)).toHaveLength(1)
    })

    // Captions are always rendered at their default position; the offset is
    // applied in the browser, so no render may carry an override.
    const frameCalls = call.mock.calls.filter(([method]) => method === 'generatePreviewFrame')
    expect(frameCalls.length).toBeGreaterThan(0)
    for (const [, params] of frameCalls) {
      expect(params?.positionOverrides).toEqual([])
    }
  })

  it('shows one filmstrip frame per on-screen block, not per segment', async () => {
    // A sentence is drawn as as many blocks as fit the frame — a word or two
    // each in karaoke. One still per sentence would show whichever of them the
    // sentence's midpoint happened to land on and hide the rest, which reads as
    // the preview showing text the editor does not.
    const cue = (groupStartMs: number, groupEndMs: number, text: string) => ({
      startMs: groupStartMs,
      endMs: groupEndMs,
      yPct: 88,
      groupStartMs,
      groupEndMs,
      anchor: 'bottom' as const,
      lines: [{ words: [{ text, isHighlighted: false }] }],
    })

    // One segment, four on-screen blocks.
    mockRust([cue(0, 500, 'HELLO'), cue(500, 1000, 'THERE'), cue(1000, 1500, 'BIG'), cue(1500, 2000, 'WORLD')])

    await act(async () => {
      render(
        <CaptionPositionPanel
          videoPath="/tmp/clip.mp4"
          segments={segments}
          style={style}
          overrides={[]}
          onOverridesChange={vi.fn()}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      )
    })

    await waitFor(() => {
      expect(screen.getAllByTitle(/^HELLO/)).toHaveLength(1)
    })
    expect(screen.getAllByTitle(/^THERE/)).toHaveLength(1)
    expect(screen.getAllByTitle(/^BIG/)).toHaveLength(1)
    expect(screen.getAllByTitle(/^WORLD/)).toHaveLength(1)
  })

  it('reports the block on the still, so the text list can mark its words', async () => {
    const cue = (groupStartMs: number, groupEndMs: number, text: string) => ({
      startMs: groupStartMs,
      endMs: groupEndMs,
      yPct: 88,
      groupStartMs,
      groupEndMs,
      anchor: 'bottom' as const,
      lines: [{ words: [{ text, isHighlighted: false }] }],
    })
    mockRust([cue(0, 1000, 'HELLO'), cue(1000, 2000, 'THERE')])
    const onSelectBlock = vi.fn()

    await act(async () => {
      render(
        <CaptionPositionPanel
          videoPath="/tmp/clip.mp4"
          segments={segments}
          style={style}
          overrides={[]}
          onOverridesChange={vi.fn()}
          onSelectBlock={onSelectBlock}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      )
    })

    // Without being clicked: the block that loads selected is the one on screen.
    await waitFor(() => {
      expect(onSelectBlock).toHaveBeenCalledWith({ startMs: 0, endMs: 1000 })
    })
  })

  it('leaves the other blocks in place when one is moved out of a shared override', async () => {
    // "Apply to all" and older sidecars can leave one range covering several
    // blocks. Moving one of them must not drag the rest along with it.
    const cue = (groupStartMs: number, groupEndMs: number, text: string) => ({
      startMs: groupStartMs,
      endMs: groupEndMs,
      yPct: 88,
      groupStartMs,
      groupEndMs,
      anchor: 'bottom' as const,
      lines: [{ words: [{ text, isHighlighted: false }] }],
    })
    mockRust([cue(0, 1000, 'HELLO'), cue(1000, 2000, 'THERE')])
    const onOverridesChange = vi.fn()

    await act(async () => {
      render(
        <CaptionPositionPanel
          videoPath="/tmp/clip.mp4"
          segments={segments}
          style={style}
          overrides={[{ startMs: 0, endMs: 2000, yPct: 30 }]}
          onOverridesChange={onOverridesChange}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      )
    })

    const stage = await screen.findByAltText('Video frame')
    const target = stage.parentElement as HTMLElement
    await act(async () => {
      target.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientY: 0 }))
      target.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientY: 100 }))
    })

    expect(onOverridesChange).toHaveBeenCalled()
    const written = onOverridesChange.mock.calls.at(-1)?.[0] as PositionOverride[]
    // The second block keeps the height it was already drawn at...
    expect(written).toContainEqual({ startMs: 1000, endMs: 2000, yPct: 30 })
    // ...and only the dragged one moves.
    const moved = written.find((o) => o.startMs === 0 && o.endMs === 1000)
    expect(moved?.yPct).toBeGreaterThan(30)
  })

  it('offsets the caption layer by the difference from its default position', async () => {
    mockRust([
      {
        startMs: 0,
        endMs: 2000,
        yPct: 88,
        groupStartMs: 0,
        groupEndMs: 2000,
        anchor: 'bottom',
        lines: [{ words: [{ text: 'HELLO', isHighlighted: false }] }],
      },
    ])

    await act(async () => {
      render(
        <CaptionPositionPanel
          videoPath="/tmp/clip.mp4"
          segments={segments}
          style={style}
          overrides={[{ startMs: 0, endMs: 2000, yPct: 30 }]}
          onOverridesChange={vi.fn()}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      )
    })

    const layer = await screen.findByAltText('Captions')
    // The 1080x1920 frame letterboxes into the 600x1000 stage, so the picture is
    // 1000px tall. Dragged from 88% to 30% of frame height: 58% of 1000px up.
    expect(layer.getAttribute('style')).toContain('translateY(-580px)')
  })

  it('re-renders a caption after its words are edited', async () => {
    // Editing a caption leaves its timings alone, so the render cache cannot be
    // keyed on timing: the edited caption would keep showing the old words.
    const cue = {
      startMs: 0,
      endMs: 2000,
      yPct: 88,
      groupStartMs: 0,
      groupEndMs: 2000,
      anchor: 'bottom' as const,
      lines: [{ words: [{ text: 'HELLO', isHighlighted: false }] }],
    }
    const call = mockRust([cue])

    let rerender!: (ui: React.ReactElement) => void
    await act(async () => {
      ;({ rerender } = render(
        <CaptionPositionPanel
          videoPath="/tmp/clip.mp4"
          segments={segments}
          style={style}
          overrides={[]}
          onOverridesChange={vi.fn()}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      ))
    })

    // Only caption-layer renders matter here: the background frame is re-fetched
    // on any change anyway, so counting it would hide a stale caption layer.
    const captionRenders = () =>
      call.mock.calls.filter(
        ([method, params]) => method === 'generatePreviewFrame' && params?.renderMode === 'captions'
      ).length

    await waitFor(() => expect(captionRenders()).toBeGreaterThan(0))
    const before = captionRenders()

    // Same timings, different words.
    const edited = [{ ...segments[0], text: 'GOODBYE THEN', words: segments[0].words }]
    await act(async () => {
      rerender(
        <CaptionPositionPanel
          videoPath="/tmp/clip.mp4"
          segments={edited}
          style={style}
          overrides={[]}
          onOverridesChange={vi.fn()}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      )
    })

    await waitFor(() => expect(captionRenders()).toBeGreaterThan(before))
  })

  it('draws the platform overlay over the picture, not the whole stage', async () => {
    platforms = ['tiktok']
    mockRust([
      {
        startMs: 0,
        endMs: 2000,
        yPct: 88,
        groupStartMs: 0,
        groupEndMs: 2000,
        anchor: 'bottom',
        lines: [{ words: [{ text: 'HELLO', isHighlighted: false }] }],
      },
    ])

    await act(async () => {
      render(
        <CaptionPositionPanel
          videoPath="/tmp/clip.mp4"
          segments={segments}
          style={style}
          overrides={[]}
          onOverridesChange={vi.fn()}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      )
    })

    // TikTok contributes four regions.
    const caption = await screen.findByText('Caption')
    expect(screen.getByText('Tabs')).toBeTruthy()
    expect(screen.getByText('Actions')).toBeTruthy()
    expect(screen.getByText('Nav')).toBeTruthy()

    // The 1080x1920 frame fills the 600x1000 stage's height and is 562.5 wide,
    // so the overlay is inset horizontally rather than spanning the stage.
    const overlay = caption.parentElement?.parentElement
    expect(overlay?.getAttribute('style')).toContain('width: 562.5px')
    expect(overlay?.getAttribute('style')).toContain('left: 18.75px')
  })

  it('measures the offset against the picture, not the box around it', async () => {
    // A wide frame leaves bars above and below. Offsetting by a share of the
    // stage height instead of the picture height would overshoot badly — and
    // sizing the stage from the ratio would push it off screen entirely.
    Object.defineProperty(window.Image.prototype, 'naturalWidth', { configurable: true, value: 1920 })
    Object.defineProperty(window.Image.prototype, 'naturalHeight', { configurable: true, value: 1080 })

    mockRust([
      {
        startMs: 0,
        endMs: 2000,
        yPct: 88,
        groupStartMs: 0,
        groupEndMs: 2000,
        anchor: 'bottom',
        lines: [{ words: [{ text: 'HELLO', isHighlighted: false }] }],
      },
    ])

    await act(async () => {
      render(
        <CaptionPositionPanel
          videoPath="/tmp/wide.mp4"
          segments={segments}
          style={style}
          overrides={[{ startMs: 0, endMs: 2000, yPct: 38 }]}
          onOverridesChange={vi.fn()}
          shownPlatforms={platforms}
          onShownPlatformsChange={vi.fn()}
        />
      )
    })

    const layer = await screen.findByAltText('Captions')
    // 1920x1080 into a 600x1000 stage scales by 600/1920, so the picture is
    // 337.5px tall. A 50-point move is 168.75px, not 500px.
    expect(layer.getAttribute('style')).toContain('translateY(-168.75px)')
  })
})
