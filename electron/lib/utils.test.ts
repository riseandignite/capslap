import { describe, it, expect } from 'vitest'
import { cn } from './utils'

describe('cn (className utility)', () => {
  it('merges simple class names', () => {
    expect(cn('foo', 'bar')).toBe('foo bar')
  })

  it('handles conditional classes', () => {
    const include = true
    const exclude = false
    expect(cn('base', include && 'included', exclude && 'excluded')).toBe('base included')
  })

  it('handles undefined and null', () => {
    expect(cn('base', undefined, null, 'end')).toBe('base end')
  })

  it('handles empty strings', () => {
    expect(cn('base', '', 'end')).toBe('base end')
  })

  it('handles arrays of classes', () => {
    expect(cn(['foo', 'bar'], 'baz')).toBe('foo bar baz')
  })

  it('handles objects with boolean values', () => {
    expect(cn({ foo: true, bar: false, baz: true })).toBe('foo baz')
  })

  it('merges tailwind classes correctly', () => {
    // tw-merge should keep the last conflicting class
    expect(cn('p-2', 'p-4')).toBe('p-4')
    expect(cn('text-red-500', 'text-blue-500')).toBe('text-blue-500')
    expect(cn('mt-2', 'mt-4')).toBe('mt-4')
  })

  it('preserves non-conflicting tailwind classes', () => {
    expect(cn('p-2', 'mx-4')).toBe('p-2 mx-4')
    expect(cn('text-sm', 'font-bold')).toBe('text-sm font-bold')
  })

  it('handles complex combinations', () => {
    const isActive = true
    const isDisabled = false

    const result = cn('base-class', isActive && 'active-class', isDisabled && 'disabled-class', {
      'conditional-true': true,
      'conditional-false': false,
    })

    expect(result).toBe('base-class active-class conditional-true')
  })

  it('returns empty string for no valid classes', () => {
    expect(cn()).toBe('')
    expect(cn(undefined, null, false)).toBe('')
  })
})
