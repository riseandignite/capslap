/**
 * Where each platform's own interface sits on top of a full-screen video.
 *
 * These are approximations read off the apps in portrait, expressed as
 * percentages of the frame so they hold at any resolution. They are not exact
 * and they move between app versions — treat them as "don't put anything
 * important here", not as a pixel contract.
 */

export type PlatformId = 'tiktok' | 'reels' | 'shorts'

export interface SafeAreaRegion {
  label: string
  /** All four are percentages of the frame. */
  top: number
  left: number
  width: number
  height: number
}

export interface Platform {
  id: PlatformId
  name: string
  /** Distinct hue so several platforms can be shown at once and told apart. */
  color: string
  regions: SafeAreaRegion[]
}

export const PLATFORMS: Platform[] = [
  {
    id: 'tiktok',
    name: 'TikTok',
    color: '#22d3ee',
    regions: [
      { label: 'Tabs', top: 0, left: 0, width: 100, height: 8 },
      { label: 'Actions', top: 42, left: 84, width: 16, height: 46 },
      { label: 'Caption', top: 78, left: 0, width: 80, height: 17 },
      { label: 'Nav', top: 95, left: 0, width: 100, height: 5 },
    ],
  },
  {
    id: 'reels',
    name: 'Instagram Reels',
    color: '#e879f9',
    regions: [
      { label: 'Header', top: 0, left: 0, width: 100, height: 8 },
      { label: 'Actions', top: 45, left: 84, width: 16, height: 40 },
      { label: 'Caption', top: 80, left: 0, width: 82, height: 12 },
      { label: 'Nav', top: 92, left: 0, width: 100, height: 8 },
    ],
  },
  {
    id: 'shorts',
    name: 'YouTube Shorts',
    color: '#fb923c',
    regions: [
      { label: 'Search', top: 0, left: 0, width: 100, height: 7 },
      { label: 'Actions', top: 40, left: 85, width: 15, height: 48 },
      { label: 'Title', top: 82, left: 0, width: 84, height: 11 },
      { label: 'Nav', top: 93, left: 0, width: 100, height: 7 },
    ],
  },
]

export function platformById(id: PlatformId | null): Platform | undefined {
  return PLATFORMS.find((platform) => platform.id === id)
}

/**
 * Vertical bands a caption should avoid, as (top, bottom) percentage pairs.
 *
 * Only regions wide enough to sit under centred text count: the side action
 * rail is narrow and to the right, so a centred caption clears it comfortably.
 */
export function blockedBands(platform: Platform, minWidthPct = 60): [number, number][] {
  return platform.regions
    .filter((region) => region.width >= minWidthPct)
    .map((region) => [region.top, region.top + region.height] as [number, number])
}

export const PLATFORM_STORAGE_KEY = 'caption-safe-areas-v1'

export function loadPlatformSelection(): PlatformId[] {
  try {
    const stored = JSON.parse(localStorage.getItem(PLATFORM_STORAGE_KEY) || '[]')
    return Array.isArray(stored) ? stored : []
  } catch {
    return []
  }
}

export function savePlatformSelection(ids: PlatformId[]) {
  try {
    localStorage.setItem(PLATFORM_STORAGE_KEY, JSON.stringify(ids))
  } catch {
    // Not worth failing over.
  }
}

/** Every stretch the selected platforms cover, ready to send to the renderer. */
export function blockedBandsFor(ids: PlatformId[]): [number, number][] {
  return PLATFORMS.filter((platform) => ids.includes(platform.id)).flatMap((platform) => blockedBands(platform))
}
