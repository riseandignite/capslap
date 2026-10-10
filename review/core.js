// Caption review: the pure parts (no DOM), shared by the page and the tests.
//
// A review file is a PyCapSlap sidecar (.capslap.json) with two extra keys:
//   video:  { name, durationMs }          which video the captions belong to
//   review: { status, reviewer, reviewedAt, comments, changes }
// Everything else (style, positionOverrides) passes through untouched, so a
// reviewed file is still a valid sidecar.

export const REVIEW_FORMAT = 'capslap-review'
export const REVIEW_VERSION = 1

/** Parses a sidecar or review file. Throws with a readable message when it isn't one. */
export function parseCaptionsFile(text) {
  let data
  try {
    data = JSON.parse(text)
  } catch {
    throw new Error('Not a captions file (invalid JSON).')
  }
  // Very old sidecars were a bare array of segments.
  if (Array.isArray(data)) data = { segments: data }
  if (!data || !Array.isArray(data.segments)) {
    throw new Error('Not a captions file (no segments).')
  }
  const segments = data.segments.map((s, i) => {
    const start = Number(s.startMs)
    const end = Number(s.endMs)
    if (!Number.isFinite(start) || !Number.isFinite(end)) {
      throw new Error(`Caption ${i + 1} has no valid times.`)
    }
    return { ...s, startMs: start, endMs: end, text: String(s.text ?? '') }
  })
  return { ...data, segments }
}

/** `mm:ss.t` (or `h:mm:ss.t` from an hour on). */
export function formatTime(ms) {
  const tenths = Math.floor(Math.max(0, ms) / 100)
  const t = tenths % 10
  const totalSeconds = Math.floor(tenths / 10)
  const s = totalSeconds % 60
  const m = Math.floor(totalSeconds / 60) % 60
  const h = Math.floor(totalSeconds / 3600)
  const mmss = `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}.${t}`
  return h ? `${h}:${mmss}` : mmss
}

/** Reads `mm:ss.t`, `h:mm:ss.t` or bare seconds; null when it isn't a time. */
export function parseTime(text) {
  const value = String(text).trim().replace(',', '.')
  if (!value) return null
  const parts = value.split(':')
  if (parts.length > 3 || parts.some((p) => p === '' || !/^\d+(\.\d+)?$/.test(p))) return null
  const seconds = parts.reduce((total, part) => total * 60 + Number(part), 0)
  return Math.round(seconds * 1000)
}

function tokensOf(text) {
  return text.split(/\s+/).filter(Boolean)
}

/**
 * Word timings for a caption whose text changed. With the same number of words each
 * keeps its timing (and its joining to the previous word); otherwise the words share
 * the caption's time evenly. Mirrors CaptionSegment.update_text in the desktop app.
 */
export function retimeWords(segment, newText) {
  const tokens = tokensOf(newText)
  if (!tokens.length) return []
  const old = Array.isArray(segment.words) ? segment.words : []
  if (old.length === tokens.length) {
    return old.map((w, i) => {
      const lead = i > 0 && !w.glueToPrevious ? ' ' : ''
      return { ...w, text: lead + tokens[i] }
    })
  }
  const duration = Math.max(1, segment.endMs - segment.startMs)
  const step = Math.floor(duration / tokens.length)
  return tokens.map((tok, i) => ({
    startMs: segment.startMs + i * step,
    endMs: i === tokens.length - 1 ? segment.endMs : segment.startMs + (i + 1) * step,
    text: (i > 0 ? ' ' : '') + tok,
  }))
}

/** Word timings stretched to a caption's new start and end. */
export function rescaleWords(words, oldStart, oldEnd, newStart, newEnd) {
  if (!Array.isArray(words) || !words.length) return words
  const oldSpan = Math.max(1, oldEnd - oldStart)
  const scale = (newEnd - newStart) / oldSpan
  const map = (ms) => Math.round(newStart + (ms - oldStart) * scale)
  return words.map((w) => ({ ...w, startMs: map(w.startMs), endMs: map(w.endMs) }))
}

/**
 * New start/end for caption `index`, kept inside its neighbours and at least
 * `minMs` long. Returns null when the times can't fit.
 */
export function clampTimes(segments, index, startMs, endMs, minMs = 100) {
  const lower = index > 0 ? segments[index - 1].endMs : 0
  const upper = index < segments.length - 1 ? segments[index + 1].startMs : Infinity
  const start = Math.max(lower, startMs)
  const end = Math.min(upper, endMs)
  if (end - start < minMs) return null
  return { startMs: start, endMs: end }
}

/** The caption on screen at `ms`, or -1. */
export function activeIndex(segments, ms) {
  return segments.findIndex((s) => s.startMs <= ms && ms < s.endMs)
}

/**
 * Edits made against `original`, as a list for the review log:
 * { index, field: 'text' | 'start' | 'end', before, after }.
 */
export function diffSegments(original, edited) {
  const changes = []
  edited.forEach((seg, index) => {
    const before = original[index]
    if (!before) return
    if (seg.text !== before.text) changes.push({ index, field: 'text', before: before.text, after: seg.text })
    if (seg.startMs !== before.startMs) changes.push({ index, field: 'start', before: before.startMs, after: seg.startMs })
    if (seg.endMs !== before.endMs) changes.push({ index, field: 'end', before: before.endMs, after: seg.endMs })
  })
  return changes
}

/**
 * The file the reviewer sends back: the original with the edited captions, their
 * comments and a log of what changed. `status` says what the editor has to do:
 * 'approved' (nothing), 'commented' (read the comments) or 'changed' (import it).
 */
export function buildReviewFile(originalFile, edited, { reviewer = '', comments = {}, now = new Date() } = {}) {
  const changes = diffSegments(originalFile.segments, edited)
  const commentList = Object.entries(comments)
    .filter(([, text]) => String(text).trim())
    .map(([index, text]) => ({ index: Number(index), text: String(text).trim() }))
    .sort((a, b) => a.index - b.index)
  return {
    ...originalFile,
    format: REVIEW_FORMAT,
    formatVersion: REVIEW_VERSION,
    segments: edited,
    review: {
      status: changes.length ? 'changed' : commentList.length ? 'commented' : 'approved',
      reviewer: reviewer.trim(),
      reviewedAt: now.toISOString(),
      comments: commentList,
      changes,
    },
  }
}

/** File name for the returned review: `talk.mp4` → `talk.mp4.reviewed.capslap.json`. */
export function reviewFileName(file) {
  const base = file.video?.name || 'captions'
  return `${base}.reviewed.capslap.json`
}

/**
 * Undo/redo over whole-review snapshots. `record` is called with the state *before*
 * a change. Typing in one field is one step, not one per keystroke: a record with the
 * same `key` as the last one within `mergeMs` is merged into it.
 */
export class History {
  constructor({ limit = 200, mergeMs = 1500 } = {}) {
    this.past = []
    this.future = []
    this.limit = limit
    this.mergeMs = mergeMs
    this.lastKey = null
    this.lastAt = 0
  }

  record(snapshot, key = null, now = Date.now()) {
    if (key !== null && key === this.lastKey && now - this.lastAt < this.mergeMs) {
      this.lastAt = now
      return
    }
    this.past.push(snapshot)
    if (this.past.length > this.limit) this.past.shift()
    this.future = []
    this.lastKey = key
    this.lastAt = now
  }

  /** The state to go back to, given the current one; null when there is none. */
  undo(current) {
    if (!this.past.length) return null
    this.future.push(current)
    this.lastKey = null
    return this.past.pop()
  }

  redo(current) {
    if (!this.future.length) return null
    this.past.push(current)
    this.lastKey = null
    return this.future.pop()
  }

  get canUndo() {
    return this.past.length > 0
  }

  get canRedo() {
    return this.future.length > 0
  }
}
