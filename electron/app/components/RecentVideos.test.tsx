import React from 'react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { RecentVideos } from './RecentVideos'

describe('RecentVideos', () => {
  beforeEach(() => {
    localStorage.clear()
    ;(
      window as unknown as {
        rust: {
          filesExist: (paths: string[]) => Promise<boolean[]>
          call: (method: string, params?: unknown) => Promise<unknown>
        }
      }
    ).rust = {
      filesExist: vi.fn(async (paths: string[]) => paths.map(() => true)),
      call: vi.fn(async () => ({ imageData: 'data:image/png;base64,thumb' })),
    }
  })

  it('renders recent videos and handles loading state', async () => {
    localStorage.setItem(
      'recent-videos-v1',
      JSON.stringify([{ path: '/path/to/test-video.mp4', openedAt: Date.now() }])
    )

    render(<RecentVideos onOpen={vi.fn()} />)

    await waitFor(() => {
      expect(screen.getByText('test-video.mp4')).toBeInTheDocument()
    })
  })
})
