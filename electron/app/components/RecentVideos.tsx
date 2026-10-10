import { useCallback, useEffect, useState } from 'react'
import { Clock, X, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface RecentVideo {
  path: string
  /** Base64 JPEG still, small enough to keep in localStorage. */
  thumbnail?: string
  openedAt: number
}

const STORAGE_KEY = 'recent-videos-v1'
/** Enough to cover a working session without turning the start screen into a file browser. */
const MAX_RECENT = 8
/** Thumbnails are stored, so they have to stay small — localStorage is a few MB in total. */
const THUMBNAIL_HEIGHT = 96

export function loadRecentVideos(): RecentVideo[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((entry) => typeof entry?.path === 'string') : []
  } catch {
    return []
  }
}

function saveRecentVideos(entries: RecentVideo[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(entries.slice(0, MAX_RECENT)))
  } catch {
    // A full quota is not worth failing an export over.
  }
}

/** Move `paths` to the front of the recent list, keeping any thumbnails already fetched. */
export function rememberRecentVideos(paths: string[]): RecentVideo[] {
  const existing = loadRecentVideos()
  const byPath = new Map(existing.map((entry) => [entry.path, entry]))
  const now = Date.now()

  const merged: RecentVideo[] = [
    ...paths.map((path) => ({ ...byPath.get(path), path, openedAt: now })),
    ...existing.filter((entry) => !paths.includes(entry.path)),
  ].slice(0, MAX_RECENT)

  saveRecentVideos(merged)
  return merged
}

export function basename(path: string): string {
  return path.split('/').pop() || path
}

export function RecentVideos({ onOpen }: { onOpen: (path: string) => void }) {
  const [entries, setEntries] = useState<RecentVideo[]>([])

  // Drop entries whose file has been moved, renamed or unmounted since.
  useEffect(() => {
    let cancelled = false

    const prune = async () => {
      const stored = loadRecentVideos()
      if (stored.length === 0) {
        setEntries([])
        return
      }
      try {
        const present = await window.rust.filesExist(stored.map((entry) => entry.path))
        if (cancelled) return
        const alive = stored.filter((_, index) => present[index] !== false)
        if (alive.length !== stored.length) saveRecentVideos(alive)
        setEntries(alive)
      } catch {
        if (!cancelled) setEntries(stored)
      }
    }

    prune()
    return () => {
      cancelled = true
    }
  }, [])

  // Fetch the missing thumbnails once, then keep them in storage.
  useEffect(() => {
    const missing = entries.filter((entry) => !entry.thumbnail)
    if (missing.length === 0) return
    let cancelled = false

    const fetchThumbnails = async () => {
      for (const entry of missing) {
        try {
          const result = (await window.rust.call('generatePreviewFrame', {
            inputVideo: entry.path,
            segments: [],
            timestampMs: 0,
            renderMode: 'clean',
            thumbnailHeight: THUMBNAIL_HEIGHT,
            exportFormat: '9:16',
            outputSize: '1080p',
            karaoke: false,
            multiline: false,
            glowEffect: false,
          })) as { imageData?: string } | null
          if (cancelled || !result?.imageData) continue

          setEntries((prev) => {
            const next = prev.map((item) =>
              item.path === entry.path ? { ...item, thumbnail: result.imageData } : item
            )
            saveRecentVideos(next)
            return next
          })
        } catch {
          // A thumbnail is a nicety; the entry still works without one.
        }
      }
    }

    fetchThumbnails()
    return () => {
      cancelled = true
    }
  }, [entries])

  const forget = useCallback((path: string) => {
    setEntries((prev) => {
      const next = prev.filter((entry) => entry.path !== path)
      saveRecentVideos(next)
      return next
    })
  }, [])

  if (entries.length === 0) return null

  return (
    <div className="w-full max-w-2xl mx-auto mt-6">
      <div className="flex items-center gap-2 mb-3 text-xs font-medium text-muted-foreground">
        <Clock className="w-3.5 h-3.5" />
        Recent
      </div>

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {entries.map((entry) => (
          <div
            key={entry.path}
            className={cn(
              'group relative flex items-center gap-2 rounded-lg border border-border/40 bg-card/40 p-2',
              'cursor-pointer transition-colors hover:border-primary/50 hover:bg-primary/5'
            )}
            onClick={() => onOpen(entry.path)}
            title={entry.path}
          >
            <div className="h-12 w-9 shrink-0 overflow-hidden rounded bg-black flex items-center justify-center">
              {entry.thumbnail ? (
                <img src={entry.thumbnail} alt="" className="h-full w-full object-cover" draggable={false} />
              ) : (
                <Loader2 className="w-3.5 h-3.5 animate-spin text-muted-foreground/50" />
              )}
            </div>

            <span className="min-w-0 flex-1 truncate text-xs text-foreground/90">{basename(entry.path)}</span>

            <button
              className="absolute right-1 top-1 rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
              onClick={(event) => {
                event.stopPropagation()
                forget(entry.path)
              }}
              title="Remove from recent"
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        ))}
      </div>
    </div>
  )
}
