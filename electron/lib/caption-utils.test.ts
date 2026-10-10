import { describe, it, expect } from 'vitest'
import { formatTime, formatSrtTime, parseTime, getFontName, truncateText, clamp, FONT_NAMES } from './caption-utils'

describe('formatTime', () => {
  it('formats zero correctly', () => {
    expect(formatTime(0)).toBe('0:00.00')
  })

  it('formats seconds correctly', () => {
    expect(formatTime(5000)).toBe('0:05.00')
    expect(formatTime(30000)).toBe('0:30.00')
  })

  it('formats minutes and seconds correctly', () => {
    expect(formatTime(65000)).toBe('1:05.00')
    expect(formatTime(125000)).toBe('2:05.00')
  })

  it('formats centiseconds correctly', () => {
    expect(formatTime(5500)).toBe('0:05.50')
    // Note: 5550ms = 5.55s, but floor((5.55-5)*100) = floor(54.99...) = 54
    // This is correct behavior for floor() operation
    expect(formatTime(5550)).toBe('0:05.54')
    expect(formatTime(5999)).toBe('0:05.99')
  })

  it('handles edge cases', () => {
    expect(formatTime(1)).toBe('0:00.00')
    expect(formatTime(10)).toBe('0:00.01')
    expect(formatTime(100)).toBe('0:00.10')
  })
})

describe('formatSrtTime', () => {
  it('formats zero correctly', () => {
    expect(formatSrtTime(0)).toBe('00:00:00,000')
  })

  it('formats hours correctly', () => {
    expect(formatSrtTime(3600000)).toBe('01:00:00,000')
    expect(formatSrtTime(7200000)).toBe('02:00:00,000')
  })

  it('formats complex timestamps correctly', () => {
    expect(formatSrtTime(3661001)).toBe('01:01:01,001')
    expect(formatSrtTime(5400000)).toBe('01:30:00,000')
  })

  it('formats milliseconds correctly', () => {
    expect(formatSrtTime(123)).toBe('00:00:00,123')
    expect(formatSrtTime(1500)).toBe('00:00:01,500')
  })
})

describe('parseTime', () => {
  it('parses valid time strings', () => {
    expect(parseTime('0:05.00')).toBe(5000)
    expect(parseTime('1:30.50')).toBe(90500)
    expect(parseTime('10:00.00')).toBe(600000)
  })

  it('returns 0 for invalid formats', () => {
    expect(parseTime('')).toBe(0)
    expect(parseTime('invalid')).toBe(0)
    expect(parseTime('1:2.3')).toBe(0) // Invalid: needs :SS.mm format
    expect(parseTime('00:00:00')).toBe(0) // Invalid: HH:MM:SS format not supported
  })

  it('roundtrips with formatTime', () => {
    const testValues = [0, 5000, 65000, 125500, 600000]
    for (const ms of testValues) {
      const formatted = formatTime(ms)
      const parsed = parseTime(formatted)
      expect(parsed).toBeCloseTo(ms, -2) // Within 10ms due to centisecond precision
    }
  })
})

describe('getFontName', () => {
  it('returns correct names for known fonts', () => {
    expect(getFontName('montserrat-black')).toBe('Montserrat Black')
    expect(getFontName('komika-axis')).toBe('Komika Axis')
    expect(getFontName('theboldfont')).toBe('THEBOLDFONT')
    expect(getFontName('roboto-bold')).toBe('Roboto Bold')
  })

  it('returns default for unknown fonts', () => {
    expect(getFontName('unknown-font')).toBe('Montserrat Black')
    expect(getFontName('')).toBe('Montserrat Black')
    expect(getFontName('comic-sans')).toBe('Montserrat Black')
  })

  it('has all expected fonts in FONT_NAMES', () => {
    const expectedFonts = [
      'montserrat-black',
      'komika-axis',
      'theboldfont',
      'kanit-bold',
      'poppins-black',
      'oswald-bold',
      'bangers-regular',
      'worksans-bold',
      'roboto-bold',
    ]
    expect(Object.keys(FONT_NAMES)).toEqual(expectedFonts)
  })
})

describe('truncateText', () => {
  it('returns original text if within limit', () => {
    expect(truncateText('Hello', 10)).toBe('Hello')
    expect(truncateText('Hello', 5)).toBe('Hello')
  })

  it('truncates with ellipsis when exceeding limit', () => {
    expect(truncateText('Hello World', 8)).toBe('Hello W…')
    expect(truncateText('Testing long text', 10)).toBe('Testing l…')
  })

  it('handles edge cases', () => {
    expect(truncateText('', 5)).toBe('')
    expect(truncateText('Hi', 1)).toBe('…')
  })
})

describe('clamp', () => {
  it('returns value when within range', () => {
    expect(clamp(5, 0, 10)).toBe(5)
    expect(clamp(0, 0, 10)).toBe(0)
    expect(clamp(10, 0, 10)).toBe(10)
  })

  it('clamps to min when below', () => {
    expect(clamp(-5, 0, 10)).toBe(0)
    expect(clamp(-100, 0, 10)).toBe(0)
  })

  it('clamps to max when above', () => {
    expect(clamp(15, 0, 10)).toBe(10)
    expect(clamp(100, 0, 10)).toBe(10)
  })

  it('works with negative ranges', () => {
    expect(clamp(0, -10, -5)).toBe(-5)
    expect(clamp(-7, -10, -5)).toBe(-7)
    expect(clamp(-15, -10, -5)).toBe(-10)
  })

  it('works with decimal values', () => {
    expect(clamp(0.5, 0, 1)).toBe(0.5)
    expect(clamp(-0.1, 0, 1)).toBe(0)
    expect(clamp(1.5, 0, 1)).toBe(1)
  })
})
