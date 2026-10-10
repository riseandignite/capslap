import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  activeIndex,
  buildReviewFile,
  clampTimes,
  diffSegments,
  formatTime,
  History,
  parseCaptionsFile,
  parseTime,
  rescaleWords,
  retimeWords,
  reviewFileName,
} from './core.js'

const sidecar = {
  video: { name: 'talk.mp4', durationMs: 9000 },
  segments: [
    {
      startMs: 0,
      endMs: 2000,
      text: 'Kylläpä on sää',
      words: [
        { startMs: 0, endMs: 600, text: 'Kylläpä' },
        { startMs: 600, endMs: 1200, text: ' on' },
        { startMs: 1200, endMs: 2000, text: ' sää' },
      ],
    },
    { startMs: 2000, endMs: 4000, text: 'tekoäly-', words: [{ startMs: 2000, endMs: 4000, text: 'tekoäly-' }] },
    { startMs: 4500, endMs: 6000, text: 'yhtiöt' },
  ],
  style: { fontName: 'Montserrat' },
  positionOverrides: [{ startMs: 0, endMs: 1000, yPct: 70 }],
}

test('parses sidecars, legacy arrays and rejects the rest', () => {
  assert.equal(parseCaptionsFile(JSON.stringify(sidecar)).segments.length, 3)
  assert.equal(parseCaptionsFile(JSON.stringify(sidecar.segments)).segments.length, 3)
  assert.throws(() => parseCaptionsFile('{nope'), /invalid JSON/)
  assert.throws(() => parseCaptionsFile('{"foo": 1}'), /no segments/)
  assert.throws(() => parseCaptionsFile('{"segments": [{"text": "x"}]}'), /Caption 1/)
})

test('formats and reads times', () => {
  assert.equal(formatTime(0), '00:00.0')
  assert.equal(formatTime(75_450), '01:15.4')
  assert.equal(formatTime(3_725_000), '1:02:05.0')
  assert.equal(parseTime('01:15.4'), 75_400)
  assert.equal(parseTime('1:02:05'), 3_725_000)
  assert.equal(parseTime('12,5'), 12_500)
  assert.equal(parseTime(' 7 '), 7_000)
  for (const bad of ['', 'abc', '1::2', '-3', '1:2:3:4']) assert.equal(parseTime(bad), null, bad)
})

test('keeps word timings when the word count stays', () => {
  const words = retimeWords(sidecar.segments[0], 'Kyllä on sää')
  assert.deepEqual(
    words.map((w) => [w.startMs, w.endMs, w.text]),
    [
      [0, 600, 'Kyllä'],
      [600, 1200, ' on'],
      [1200, 2000, ' sää'],
    ]
  )
})

test('keeps a syllable glued to the word before it', () => {
  const seg = { startMs: 0, endMs: 1000, words: [{ startMs: 0, endMs: 500, text: 'teko' }, { startMs: 500, endMs: 1000, text: 'äly', glueToPrevious: true }] }
  assert.deepEqual(retimeWords(seg, 'tekö äly').map((w) => w.text), ['tekö', 'äly'])
})

test('shares the time evenly when the word count changes', () => {
  const words = retimeWords(sidecar.segments[0], 'Onpa tänään kaunis sää')
  assert.equal(words.length, 4)
  assert.equal(words[0].startMs, 0)
  assert.equal(words[3].endMs, 2000)
  assert.equal(words[1].text, ' tänään')
  assert.deepEqual(retimeWords(sidecar.segments[0], '   '), [])
})

test('stretches word timings to new caption times', () => {
  const words = rescaleWords(sidecar.segments[0].words, 0, 2000, 1000, 2000)
  assert.deepEqual(words.map((w) => [w.startMs, w.endMs]), [[1000, 1300], [1300, 1600], [1600, 2000]])
})

test('keeps edited times inside the neighbours', () => {
  const segs = sidecar.segments
  assert.deepEqual(clampTimes(segs, 1, 1500, 5000), { startMs: 2000, endMs: 4500 })
  assert.deepEqual(clampTimes(segs, 0, 0, 1800), { startMs: 0, endMs: 1800 })
  assert.equal(clampTimes(segs, 1, 3000, 3050), null)
  assert.deepEqual(clampTimes(segs, 2, 4600, 99_000), { startMs: 4600, endMs: 99_000 })
})

test('finds the caption on screen', () => {
  assert.equal(activeIndex(sidecar.segments, 100), 0)
  assert.equal(activeIndex(sidecar.segments, 2000), 1)
  assert.equal(activeIndex(sidecar.segments, 4200), -1)
})

test('logs text and time changes by caption', () => {
  const edited = structuredClone(sidecar.segments)
  edited[0].text = 'Kyllä on sää'
  edited[2].startMs = 4400
  assert.deepEqual(diffSegments(sidecar.segments, edited), [
    { index: 0, field: 'text', before: 'Kylläpä on sää', after: 'Kyllä on sää' },
    { index: 2, field: 'start', before: 4500, after: 4400 },
  ])
})

test('builds the file to send back, keeping the rest of the sidecar', () => {
  const now = new Date('2026-09-25T12:00:00Z')
  const untouched = buildReviewFile(sidecar, structuredClone(sidecar.segments), { reviewer: ' Asiakas ', now })
  assert.equal(untouched.review.status, 'approved')
  assert.equal(untouched.review.reviewer, 'Asiakas')
  assert.equal(untouched.review.reviewedAt, '2026-09-25T12:00:00.000Z')
  assert.deepEqual(untouched.style, sidecar.style)
  assert.deepEqual(untouched.positionOverrides, sidecar.positionOverrides)
  assert.equal(untouched.format, 'capslap-review')

  const commented = buildReviewFile(sidecar, structuredClone(sidecar.segments), { comments: { 2: ' tarkista nimi ', 0: '  ' } })
  assert.equal(commented.review.status, 'commented')
  assert.deepEqual(commented.review.comments, [{ index: 2, text: 'tarkista nimi' }])

  const edited = structuredClone(sidecar.segments)
  edited[1].text = 'tekoäly'
  const changed = buildReviewFile(sidecar, edited)
  assert.equal(changed.review.status, 'changed')
  assert.equal(changed.segments[1].text, 'tekoäly')
})

test('names the file after the video', () => {
  assert.equal(reviewFileName(sidecar), 'talk.mp4.reviewed.capslap.json')
  assert.equal(reviewFileName({}), 'captions.reviewed.capslap.json')
})

test('undoes and redoes whole steps', () => {
  const h = new History()
  h.record('a', null, 0)
  h.record('b', null, 10)
  assert.equal(h.undo('c'), 'b')
  assert.equal(h.undo('b'), 'a')
  assert.equal(h.undo('a'), null)
  assert.equal(h.redo('a'), 'b')
  assert.equal(h.redo('b'), 'c')
  assert.equal(h.redo('c'), null)
})

test('merges typing in one field into one step', () => {
  const h = new History({ mergeMs: 1000 })
  h.record('before', 'text:0', 0)
  h.record('k1', 'text:0', 300)
  h.record('k2', 'text:0', 900)
  h.record('other', 'text:1', 950)
  h.record('later', 'text:1', 5000)
  assert.deepEqual(h.past, ['before', 'other', 'later'])
})

test('a new change drops the redo steps', () => {
  const h = new History()
  h.record('a', null, 0)
  h.undo('b')
  assert.ok(h.canRedo)
  h.record('a2', null, 10)
  assert.ok(!h.canRedo)
})

test('keeps at most `limit` steps', () => {
  const h = new History({ limit: 2 })
  for (const s of ['a', 'b', 'c']) h.record(s)
  assert.deepEqual(h.past, ['b', 'c'])
})
