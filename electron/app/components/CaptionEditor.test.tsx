import React from 'react'
import { describe, it, expect, vi } from 'vitest'
import { render, screen, act, waitFor } from '@testing-library/react'
import { CaptionEditor } from './CaptionEditor'

const mockSettings = {
  selectedTemplate: 'oneliner' as const,
  exportFormats: ['9:16'],
  selectedFont: 'Montserrat Black',
  textColor: '#ffffff',
  highlightWordColor: '#ffff00',
  outlineColor: '#000000',
  glowEffect: false,
  captionStyle: 'oneliner' as const,
  captionPosition: 'bottom' as const,
  fontSize: 65,
}

const mockSegments = [
  {
    startMs: 0,
    endMs: 2000,
    text: 'Hello world',
    words: [
      { startMs: 0, endMs: 1000, text: 'Hello' },
      { startMs: 1000, endMs: 2000, text: 'world' },
    ],
  },
]

describe('CaptionEditor', () => {
  it('renders loading indicator when isLoadingPreview is true and previewFrame is null', () => {
    render(
      <CaptionEditor
        initialSegments={mockSegments}
        onBurn={vi.fn()}
        onCancel={vi.fn()}
        videoPath="test.mp4"
        settings={mockSettings}
        fontName="Montserrat Black"
        positionOverrides={[]}
        onPositionOverridesChange={vi.fn()}
        shownPlatforms={[]}
        onShownPlatformsChange={vi.fn()}
        previewFrame={null}
        isLoadingPreview={true}
      />
    )

    expect(screen.getByText('Loading preview frame…')).toBeInTheDocument()
  })

  it('renders preview frame image when previewFrame is provided', () => {
    const testImage = 'data:image/png;base64,mockframe'
    render(
      <CaptionEditor
        initialSegments={mockSegments}
        onBurn={vi.fn()}
        onCancel={vi.fn()}
        videoPath="test.mp4"
        settings={mockSettings}
        fontName="Montserrat Black"
        positionOverrides={[]}
        onPositionOverridesChange={vi.fn()}
        shownPlatforms={[]}
        onShownPlatformsChange={vi.fn()}
        previewFrame={testImage}
        isLoadingPreview={false}
      />
    )

    const img = screen.getByAltText('Preview')
    expect(img).toBeInTheDocument()
    expect(img).toHaveAttribute('src', testImage)
  })

  it('marks the words that are on the still while positioning', async () => {
    // The still shows a caption a few words at a time, so a sentence in the list
    // is only partly on screen. Without this the preview looks like it is
    // rendering text the editor does not have.
    const PIXEL =
      'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=='
    const cue = (groupStartMs: number, groupEndMs: number, text: string) => ({
      startMs: groupStartMs,
      endMs: groupEndMs,
      yPct: 88,
      groupStartMs,
      groupEndMs,
      anchor: 'bottom' as const,
      lines: [{ words: [{ text, isHighlighted: false }] }],
    })
    ;(window as unknown as { rust: { call: unknown } }).rust = {
      call: vi.fn(async (method: string) => {
        if (method === 'generatePreviewFrame') return { imageData: PIXEL }
        if (method === 'previewLayout') return { cues: [cue(0, 1000, 'HELLO'), cue(1000, 2000, 'WORLD')] }
        return null
      }),
    }
    class Eager {
      constructor(private callback: (entries: unknown[], observer: unknown) => void) {}
      observe(target: Element) {
        this.callback([{ isIntersecting: true, target, contentRect: { width: 600, height: 1000 } }], this)
      }
      unobserve() {}
      disconnect() {}
      takeRecords() {
        return []
      }
    }
    global.IntersectionObserver = Eager as unknown as typeof IntersectionObserver
    global.ResizeObserver = Eager as unknown as typeof ResizeObserver
    Object.defineProperty(window.Image.prototype, 'naturalWidth', { configurable: true, value: 1080 })
    Object.defineProperty(window.Image.prototype, 'naturalHeight', { configurable: true, value: 1920 })
    Object.defineProperty(window.Image.prototype, 'src', {
      configurable: true,
      set() {
        setTimeout(() => this.onload?.(new Event('load')), 0)
      },
    })

    await act(async () => {
      render(
        <CaptionEditor
          initialSegments={mockSegments}
          onBurn={vi.fn()}
          onCancel={vi.fn()}
          videoPath="test.mp4"
          settings={mockSettings}
          fontName="Montserrat Black"
          positionOverrides={[]}
          onPositionOverridesChange={vi.fn()}
          shownPlatforms={[]}
          onShownPlatformsChange={vi.fn()}
          previewFrame={null}
          isLoadingPreview={false}
        />
      )
    })

    await act(async () => {
      screen.getByTitle('Drag captions to a different height').click()
    })

    // The first block covers "Hello" only, so "world" stays unmarked.
    await waitFor(() => {
      expect(screen.getByText('Hello').getAttribute('title')).toContain('on the still')
    })
    expect(screen.getByText('world').getAttribute('title')).not.toContain('on the still')
  })
})
