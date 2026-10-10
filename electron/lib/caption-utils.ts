/**
 * Font name mappings for display
 */
export const FONT_NAMES = {
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

/**
 * Get display name for a font ID
 */
export function getFontName(fontId: string): string {
  return FONT_NAMES[fontId as keyof typeof FONT_NAMES] || 'Montserrat Black'
}

/**
 * Format milliseconds into a time string (M:SS.mm)
 */
export function formatTime(ms: number): string {
  const s = ms / 1000
  const minutes = Math.floor(s / 60)
  const seconds = Math.floor(s % 60)
  const millis = Math.floor((s % 1) * 100)
  return `${minutes}:${seconds.toString().padStart(2, '0')}.${millis.toString().padStart(2, '0')}`
}

/**
 * Format milliseconds into SRT-style timestamp (HH:MM:SS,mmm)
 */
export function formatSrtTime(ms: number): string {
  const hours = Math.floor(ms / 3600000)
  const minutes = Math.floor((ms % 3600000) / 60000)
  const seconds = Math.floor((ms % 60000) / 1000)
  const millis = ms % 1000
  return `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')},${millis.toString().padStart(3, '0')}`
}

/**
 * Parse a time string (M:SS.mm or MM:SS.mm) back to milliseconds
 */
export function parseTime(timeStr: string): number {
  const match = timeStr.match(/^(\d+):(\d{2})\.(\d{2})$/)
  if (!match) return 0

  const minutes = parseInt(match[1], 10)
  const seconds = parseInt(match[2], 10)
  const centis = parseInt(match[3], 10)

  return (minutes * 60 + seconds) * 1000 + centis * 10
}

/**
 * Truncate text to a maximum length with ellipsis
 */
export function truncateText(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text
  return text.slice(0, maxLength - 1) + '…'
}

/**
 * Clamp a number between min and max
 */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}
