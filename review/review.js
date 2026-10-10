import {
  activeIndex,
  buildReviewFile,
  clampTimes,
  formatTime,
  History,
  parseCaptionsFile,
  parseTime,
  rescaleWords,
  retimeWords,
  reviewFileName,
} from './core.js'
import { isLocked, lockText, unlock, WrongPassword } from './lock.js'

// ── Language: Finnish for Finnish browsers, English otherwise ────────────────

const STRINGS = {
  en: {},
  fi: {
    title: 'Tekstitysten tarkistus',
    reviewer: 'Nimesi',
    download: 'Lataa tarkistettu tiedosto',
    loadTitle: 'Tarkista tekstitykset',
    step1: 'Avaa video ja tekstitystiedosto, jotka sait.',
    step2: 'Katso video. Korjaa väärät tekstit tai jätä kommentti.',
    step3: 'Lataa tarkistettu tiedosto ja lähetä se takaisin.',
    dropHint: 'Pudota video ja .capslap.json-tiedosto tähän tai valitse ne:',
    pickVideo: 'Valitse video',
    pickCaptions: 'Valitse tekstitykset',
    privacy: 'Mitään ei lähetetä minnekään: video ja tekstit pysyvät tällä koneella.',
    keys: 'Välilyönti toisto/tauko · Alt+↑/↓ edellinen/seuraava teksti · Ctrl+Enter toista muokattava teksti · Ctrl/⌘+Z kumoa, Vaihto+Ctrl/⌘+Z tee uudelleen',
    restored: 'Keskeneräinen tarkistuksesi palautettiin.',
    startOver: 'Aloita alusta',
    changed: 'muutettu',
    revert: 'Kumoa',
    comment: 'Kommentti',
    playCue: 'Toista tämä teksti',
    notePlaceholder: 'Kommentti tekijälle (ei näy videossa)',
    summaryNone: 'Ei muutoksia',
    summaryChanges: (n) => `${n} ${n === 1 ? 'muutos' : 'muutosta'}`,
    summaryComments: (n) => `${n} ${n === 1 ? 'kommentti' : 'kommenttia'}`,
    downloaded: 'Tiedosto ladattu. Lähetä se takaisin tekijälle.',
    wrongVideo: (name) => `Huom: tekstitykset on tehty videolle ${name}.`,
    notCaptions: 'Tämä ei ole tekstitystiedosto.',
    passwordLabel: 'Tekstitykset on lukittu. Salasana (sait sen erikseen):',
    unlockButton: 'Avaa',
    back: '← Takaisin',
    send: 'Lähetä tekijälle',
    sending: 'Lähetetään…',
    sentToEditor: 'Lähetetty. Kiitos!',
    sendFailed: 'Lähetys epäonnistui. Lataa tiedosto ja lähetä se muuten.',
    downloadSecondary: 'Lataa tiedostona',
    wrongPassword: 'Väärä salasana.',
    unlocking: 'Avataan…',
  },
}
const EN = {
  playCue: 'Play this caption',
  notePlaceholder: 'Comment for the editor (not shown in the video)',
  summaryNone: 'No changes',
  summaryChanges: (n) => `${n} ${n === 1 ? 'change' : 'changes'}`,
  summaryComments: (n) => `${n} ${n === 1 ? 'comment' : 'comments'}`,
  downloaded: 'File downloaded. Send it back to the editor.',
  wrongVideo: (name) => `Note: these captions were made for ${name}.`,
  notCaptions: 'This is not a captions file.',
  wrongPassword: 'Wrong password.',
  sending: 'Sending…',
  sentToEditor: 'Sent. Thank you!',
  sendFailed: 'Sending failed. Download the file and send it another way.',
  downloadSecondary: 'Download as a file',
  unlocking: 'Opening…',
}
const lang = navigator.language?.toLowerCase().startsWith('fi') ? 'fi' : 'en'
const t = (key, ...args) => {
  const value = STRINGS[lang][key] ?? EN[key]
  return typeof value === 'function' ? value(...args) : value
}
if (lang !== 'en') {
  document.documentElement.lang = lang
  document.title = `${t('title')} · PyCapSlap`
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    const text = STRINGS[lang][el.dataset.i18n]
    if (typeof text === 'string') el.textContent = text
  })
}

// ── State ─────────────────────────────────────────────────────────────────────

const $ = (id) => document.getElementById(id)
const video = $('video')
const state = {
  file: null, // the captions file as received
  segments: [], // edited copy
  comments: {}, // index → text
  videoName: '',
  videoUrl: null,
  stopAtMs: null, // "play this caption" stops here
  storageKey: null,
  lock: null, // key of a password-locked file: the draft and the reply are locked too
  videoFromLink: false, // a linked video's URL says nothing about its file name
  submitUrl: null, // the review portal takes the reviewed file here (?submit=)
}
const history = new History()

// ── Undo ──────────────────────────────────────────────────────────────────────

function snapshot() {
  return structuredClone({ segments: state.segments, comments: state.comments })
}

// Call before changing the review. Typing in one field (same key) is one step.
function remember(key = null) {
  history.record(snapshot(), key)
}

function restore(step) {
  if (!step) return
  // Put the cursor back where it was, in the same caption and box.
  const focused = document.activeElement
  const cue = focused?.closest?.('.cue')
  const index = cue ? Number(cue.dataset.index) : null
  const field = ['text', 'note', 'start', 'end'].find((c) => focused?.classList?.contains(c))
  state.segments = step.segments
  state.comments = step.comments
  renderCues()
  if (index !== null && field) {
    const el = cueItem(index)?.querySelector(`.${field}`)
    if (el && !el.hidden) {
      el.focus()
      if (typeof el.value === 'string') el.setSelectionRange?.(el.value.length, el.value.length)
    }
  }
  updateSummary()
  showCaption()
  saveDraft()
}

function undo() {
  restore(history.undo(snapshot()))
}

function redo() {
  restore(history.redo(snapshot()))
}

// ── Loading ───────────────────────────────────────────────────────────────────

function loadVideo(file) {
  if (state.videoUrl) URL.revokeObjectURL(state.videoUrl)
  state.videoUrl = URL.createObjectURL(file)
  state.videoName = file.name
  $('video-status').textContent = file.name
  video.src = state.videoUrl
  maybeStart()
}

async function loadCaptions(file) {
  await openCaptions(await file.text(), file.name)
}

// A locked file waits here for its password.
let lockedFile = null

async function openCaptions(text, name) {
  try {
    let data
    try {
      data = JSON.parse(text)
    } catch {
      data = null
    }
    if (isLocked(data)) {
      lockedFile = { data, name }
      $('unlock').hidden = false
      $('password').focus()
      $('load-error').textContent = ''
      return
    }
    state.lock = null
    setCaptions(parseCaptionsFile(text))
    $('captions-status').textContent = name
    $('load-error').textContent = ''
  } catch (err) {
    $('load-error').textContent = `${t('notCaptions')} ${err.message}`
  }
}

$('unlock').addEventListener('submit', async (e) => {
  e.preventDefault()
  if (!lockedFile) return
  const button = $('unlock').querySelector('button')
  button.disabled = true
  $('load-error').textContent = t('unlocking')
  try {
    const opened = await unlock(lockedFile.data, $('password').value)
    const file = parseCaptionsFile(opened.text)
    state.lock = opened.lock
    $('password').value = ''
    $('unlock').hidden = true
    $('captions-status').textContent = lockedFile.name
    $('load-error').textContent = ''
    lockedFile = null
    setCaptions(file)
  } catch (err) {
    $('load-error').textContent = err instanceof WrongPassword ? t('wrongPassword') : `${t('notCaptions')} ${err.message}`
    $('password').select()
  } finally {
    button.disabled = false
  }
})

function setCaptions(file) {
  state.file = file
  state.segments = structuredClone(file.segments)
  state.comments = {}
  maybeStart()
}

function handleFiles(files) {
  for (const file of files) {
    if (file.type.startsWith('video/') || /\.(mp4|mov|m4v|webm|mkv)$/i.test(file.name)) loadVideo(file)
    else loadCaptions(file)
  }
}

async function maybeStart() {
  if (!state.file || !video.src) return
  $('loader').hidden = true
  $('workspace').hidden = false
  $('download').disabled = false
  $('send').disabled = false
  const expected = state.file.video?.name
  const name = expected || state.videoName
  $('file-name').textContent = name
  if (expected && state.videoName && !state.videoFromLink && expected !== state.videoName) {
    $('file-name').textContent = `${state.videoName} — ${t('wrongVideo', expected)}`
  }
  state.storageKey = `capslap-review:${name}:${fingerprint(state.file.segments)}`
  await restoreDraft()
  renderCues()
  updateSummary()
}

// Captions can also come from links: ?video=URL&captions=URL
async function loadFromQuery() {
  const params = new URLSearchParams(location.search)
  const captionsUrl = params.get('captions')
  const videoUrl = params.get('video')
  // Opened from the review portal: send the result there, and offer the way back.
  const submit = params.get('submit')
  const back = params.get('back')
  if (submit && sameSite(submit)) {
    state.submitUrl = submit
    $('send').hidden = false
    $('download').className = 'secondary'
    $('download').textContent = t('downloadSecondary')
  }
  if (back && sameSite(back)) {
    $('back').href = back
    $('back').hidden = false
  }
  try {
    if (captionsUrl) {
      const response = await fetch(captionsUrl)
      await openCaptions(await response.text(), decodeURIComponent(captionsUrl.split('/').pop()))
    }
  } catch (err) {
    $('load-error').textContent = `${t('notCaptions')} ${err.message}`
  }
  if (videoUrl) {
    state.videoName = decodeURIComponent(videoUrl.split('/').pop().split('?')[0])
    state.videoFromLink = true
    $('video-status').textContent = state.videoName
    video.src = videoUrl
    maybeStart()
  }
}

// Only this site's own addresses: a link must not make the page post a review elsewhere.
function sameSite(url) {
  try {
    return new URL(url, location.href).origin === location.origin
  } catch {
    return false
  }
}

// ── Drafts survive a closed tab ───────────────────────────────────────────────

function fingerprint(segments) {
  let hash = 0
  for (const s of segments) {
    for (const ch of `${s.startMs}|${s.text}`) hash = (hash * 31 + ch.charCodeAt(0)) | 0
  }
  return (hash >>> 0).toString(36)
}

// A draft of a locked file is locked with the same key. Saves run one after another,
// so a slow one can't overwrite a newer one.
let saving = Promise.resolve()

function saveDraft() {
  const draft = JSON.stringify({ segments: state.segments, comments: state.comments, reviewer: $('reviewer').value })
  const key = state.storageKey
  const lock = state.lock
  saving = saving.then(async () => {
    try {
      localStorage.setItem(key, lock ? JSON.stringify(await lockText(draft, lock)) : draft)
    } catch {
      // Private windows and full storage: the review still works, just isn't kept.
    }
  })
}

async function restoreDraft() {
  try {
    let saved = JSON.parse(localStorage.getItem(state.storageKey) || 'null')
    if (isLocked(saved)) {
      // Only the password this file was opened with opens its draft.
      saved = state.lock ? JSON.parse((await unlock(saved, '', state.lock)).text) : null
    }
    if (saved?.segments?.length === state.file.segments.length) {
      state.segments = saved.segments
      state.comments = saved.comments || {}
      if (saved.reviewer) $('reviewer').value = saved.reviewer
      $('restored').hidden = false
    }
  } catch {
    // Unreadable draft: start from the file.
  }
  try {
    const name = localStorage.getItem('capslap-review:reviewer')
    if (name && !$('reviewer').value) $('reviewer').value = name
  } catch {
    // ignore
  }
}

$('start-over').addEventListener('click', () => {
  remember()
  try {
    localStorage.removeItem(state.storageKey)
  } catch {
    // ignore
  }
  state.segments = structuredClone(state.file.segments)
  state.comments = {}
  $('restored').hidden = true
  renderCues()
  updateSummary()
})

// ── Caption list ──────────────────────────────────────────────────────────────

const list = $('cue-list')
const template = $('cue-template')

function renderCues() {
  list.replaceChildren()
  state.segments.forEach((seg, index) => {
    const li = template.content.firstElementChild.cloneNode(true)
    li.dataset.index = index
    if (lang !== 'en') {
      li.querySelectorAll('[data-i18n]').forEach((el) => {
        const text = STRINGS[lang][el.dataset.i18n]
        if (typeof text === 'string') el.textContent = text
      })
    }
    const text = li.querySelector('.text')
    const note = li.querySelector('.note')
    const start = li.querySelector('.start')
    const end = li.querySelector('.end')
    li.querySelector('.play-cue').title = t('playCue')
    note.placeholder = t('notePlaceholder')

    text.value = seg.text
    start.value = formatTime(seg.startMs)
    end.value = formatTime(seg.endMs)
    if (state.comments[index]) {
      note.value = state.comments[index]
      note.hidden = false
    }

    text.addEventListener('input', () => {
      remember(`text:${index}`)
      editText(index, text.value)
      autoGrow(text)
    })
    text.addEventListener('focus', () => seek(seg.startMs, false))
    text.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault()
        playCue(index)
      }
    })
    for (const input of [start, end]) {
      input.addEventListener('change', () => editTimes(index, li))
    }
    note.addEventListener('input', () => {
      remember(`note:${index}`)
      state.comments[index] = note.value
      markCue(li, index)
      updateSummary()
      saveDraft()
    })
    li.querySelector('.play-cue').addEventListener('click', () => playCue(index))
    li.querySelector('.revert').addEventListener('click', () => revertCue(index))
    li.querySelector('.comment-toggle').addEventListener('click', () => {
      note.hidden = !note.hidden
      if (!note.hidden) note.focus()
    })
    markCue(li, index)
    list.append(li)
  })
  // A textarea measures its content only once it is on the page.
  list.querySelectorAll('.text').forEach(autoGrow)
  highlightActive()
}

window.addEventListener('resize', () => list.querySelectorAll('.text').forEach(autoGrow))

function autoGrow(textarea) {
  textarea.style.height = 'auto'
  textarea.style.height = `${textarea.scrollHeight + 2}px`
}

function cueItem(index) {
  return list.children[index]
}

function markCue(li, index) {
  const seg = state.segments[index]
  const original = state.file.segments[index]
  const changed = seg.text !== original.text || seg.startMs !== original.startMs || seg.endMs !== original.endMs
  li.classList.toggle('changed', changed)
  li.classList.toggle('has-note', Boolean(state.comments[index]?.trim()))
}

function editText(index, value) {
  const seg = state.segments[index]
  // Line breaks in the box are just wrapping; a caption is one line of words.
  const text = value.replace(/\s*\n\s*/g, ' ')
  seg.text = text
  seg.words = wordsFor(index)
  markCue(cueItem(index), index)
  updateSummary()
  showCaption()
  saveDraft()
}

// Word timings for caption `index` as it now stands: the original words stretched to
// its current times, re-split when its text changed. None if it never had any.
function wordsFor(index) {
  const seg = state.segments[index]
  const original = state.file.segments[index]
  if (!original.words?.length) return undefined
  const words = rescaleWords(original.words, original.startMs, original.endMs, seg.startMs, seg.endMs)
  return seg.text === original.text ? words : retimeWords({ ...seg, words }, seg.text)
}

function editTimes(index, li) {
  const start = li.querySelector('.start')
  const end = li.querySelector('.end')
  const seg = state.segments[index]
  const startMs = parseTime(start.value)
  const endMs = parseTime(end.value)
  const fitted = startMs !== null && endMs !== null ? clampTimes(state.segments, index, startMs, endMs) : null
  start.classList.toggle('invalid', !fitted)
  end.classList.toggle('invalid', !fitted)
  if (!fitted) return
  if (fitted.startMs === seg.startMs && fitted.endMs === seg.endMs) return
  remember()
  seg.startMs = fitted.startMs
  seg.endMs = fitted.endMs
  seg.words = wordsFor(index)
  start.value = formatTime(seg.startMs)
  end.value = formatTime(seg.endMs)
  markCue(li, index)
  updateSummary()
  saveDraft()
}

function revertCue(index) {
  remember()
  state.segments[index] = structuredClone(state.file.segments[index])
  const li = cueItem(index)
  li.querySelector('.text').value = state.segments[index].text
  li.querySelector('.start').value = formatTime(state.segments[index].startMs)
  li.querySelector('.end').value = formatTime(state.segments[index].endMs)
  li.querySelectorAll('.time').forEach((el) => el.classList.remove('invalid'))
  autoGrow(li.querySelector('.text'))
  markCue(li, index)
  updateSummary()
  showCaption()
  saveDraft()
}

function updateSummary() {
  const file = currentReview()
  const parts = []
  if (file.review.changes.length) {
    const cues = new Set(file.review.changes.map((c) => c.index)).size
    parts.push(t('summaryChanges', cues))
  }
  if (file.review.comments.length) parts.push(t('summaryComments', file.review.comments.length))
  $('summary').textContent = parts.length ? parts.join(' · ') : t('summaryNone')
}

function currentReview() {
  return buildReviewFile(state.file, state.segments, {
    reviewer: $('reviewer').value,
    comments: state.comments,
  })
}

// ── Playback ──────────────────────────────────────────────────────────────────

function seek(ms, play) {
  video.currentTime = ms / 1000
  if (play) video.play()
}

function playCue(index) {
  const seg = state.segments[index]
  state.stopAtMs = seg.endMs
  seek(seg.startMs, true)
}

let lastActive = -1
function highlightActive() {
  const index = activeIndex(state.segments, video.currentTime * 1000)
  if (index === lastActive) return
  cueItem(lastActive)?.classList.remove('active')
  lastActive = index
  const li = cueItem(index)
  if (!li) return
  li.classList.add('active')
  // Follow playback, but don't yank the list while someone is typing in it.
  if (!video.paused && !list.contains(document.activeElement)) {
    li.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }
}

function showCaption() {
  const index = activeIndex(state.segments, video.currentTime * 1000)
  $('caption').textContent = index >= 0 ? state.segments[index].text : ''
}

function tick() {
  if (state.stopAtMs !== null && video.currentTime * 1000 >= state.stopAtMs) {
    video.pause()
    state.stopAtMs = null
  }
  showCaption()
  highlightActive()
  if (!video.paused) requestAnimationFrame(tick)
}

video.addEventListener('play', () => requestAnimationFrame(tick))
video.addEventListener('seeked', tick)
video.addEventListener('pause', () => {
  tick()
})

// ── Keys ──────────────────────────────────────────────────────────────────────

document.addEventListener('keydown', (e) => {
  if ($('workspace').hidden) return
  // One undo for the whole review: a text box's own undo would only know its box,
  // and would miss time edits, comments and Undo-button reverts.
  const mod = e.ctrlKey || e.metaKey
  const key = e.key.toLowerCase()
  if (mod && !e.altKey && (key === 'z' || key === 'y')) {
    if (document.activeElement?.id === 'reviewer') return
    e.preventDefault()
    if (key === 'y' || e.shiftKey) redo()
    else undo()
    return
  }
  const typing = ['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName)
  if (e.key === ' ' && !typing) {
    e.preventDefault()
    if (video.paused) video.play()
    else video.pause()
  }
  if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
    e.preventDefault()
    const current = document.activeElement?.closest?.('.cue')
    const from = current ? Number(current.dataset.index) : Math.max(0, lastActive)
    const next = Math.min(state.segments.length - 1, Math.max(0, from + (e.key === 'ArrowUp' ? -1 : 1)))
    const text = cueItem(next)?.querySelector('.text')
    text?.focus()
    text?.scrollIntoView({ block: 'nearest' })
  }
})

// ── Download ──────────────────────────────────────────────────────────────────

$('reviewer').addEventListener('input', () => {
  try {
    localStorage.setItem('capslap-review:reviewer', $('reviewer').value)
  } catch {
    // ignore
  }
  if (state.file) saveDraft()
})

$('download').addEventListener('click', async () => {
  const file = currentReview()
  // A locked file goes back locked, with the password it came with.
  const out = state.lock ? await lockText(JSON.stringify(file), state.lock) : file
  const blob = new Blob([JSON.stringify(out, null, 2)], { type: 'application/json' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = reviewFileName(file.video ? file : { video: { name: state.videoName } })
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 1000)
  $('summary').textContent = t('downloaded')
})

$('send').addEventListener('click', async () => {
  const button = $('send')
  button.disabled = true
  $('summary').textContent = t('sending')
  try {
    const file = currentReview()
    const response = await fetch(state.submitUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      // The portal link is the protection here; the server reads the captions.
      body: JSON.stringify(file),
    })
    if (!response.ok) throw new Error(response.statusText)
    $('summary').textContent = t('sentToEditor')
    try {
      // Sent is done: the next visit starts from what the editor has.
      localStorage.removeItem(state.storageKey)
    } catch {
      // ignore
    }
  } catch {
    $('summary').textContent = t('sendFailed')
  } finally {
    button.disabled = false
  }
})

// ── Wiring ────────────────────────────────────────────────────────────────────

$('pick-video').addEventListener('change', (e) => e.target.files[0] && loadVideo(e.target.files[0]))
$('pick-captions').addEventListener('change', (e) => e.target.files[0] && loadCaptions(e.target.files[0]))

const drop = $('drop')
for (const target of [drop, document.body]) {
  target.addEventListener('dragover', (e) => {
    e.preventDefault()
    drop.classList.add('over')
  })
  target.addEventListener('dragleave', () => drop.classList.remove('over'))
  target.addEventListener('drop', (e) => {
    e.preventDefault()
    drop.classList.remove('over')
    handleFiles(e.dataTransfer.files)
  })
}

loadFromQuery()
