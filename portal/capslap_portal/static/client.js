// The client side of the portal: a series' episodes, an episode's videos,
// caption checks and feedback. Everything is reached through the series link.

import { h } from './dom.js'
import { postsEditor } from './posts.js'

const STRINGS = {
  fi: {
    episodes: 'Jaksot',
    noEpisodes: 'Jaksoja ei ole vielä.',
    videos: (n) => `${n} ${n === 1 ? 'video' : 'videota'}`,
    notes: (n) => `${n} ${n === 1 ? 'kommentti' : 'kommenttia'}`,
    back: '← Kaikki jaksot',
    noVideos: 'Videoita ei ole vielä.',
    download: 'Lataa',
    checkCaptions: 'Tarkista tekstitykset',
    reviewedBy: (who) => `Tarkistanut ${who || 'nimetön'}`,
    commentHere: 'Kommentoi tätä kohtaa',
    feedback: 'Palaute',
    noFeedback: 'Ei palautetta vielä.',
    yourName: 'Nimesi',
    checkTexts: 'Tarkista myös julkaisutekstit: ne lähtevät videon mukana jokaiseen palveluun.',
    feedbackPlaceholder: 'Mitä haluat muuttaa tai sanoa?',
    about: 'Koskee',
    wholeEpisode: 'koko jaksoa',
    atTime: (label, t) => `${label} kohdassa ${t}`,
    send: 'Lähetä',
    sent: 'Kiitos, palaute tallennettiin.',
    anonymous: 'Nimetön',
    failed: 'Jokin meni vikaan. Yritä uudelleen.',
  },
}
const EN = {
  episodes: 'Episodes',
  noEpisodes: 'No episodes yet.',
  videos: (n) => `${n} ${n === 1 ? 'video' : 'videos'}`,
  notes: (n) => `${n} ${n === 1 ? 'comment' : 'comments'}`,
  back: '← All episodes',
  noVideos: 'No videos yet.',
  download: 'Download',
  checkCaptions: 'Check captions',
  reviewedBy: (who) => `Checked by ${who || 'someone'}`,
  commentHere: 'Comment on this moment',
  feedback: 'Feedback',
  noFeedback: 'No feedback yet.',
  yourName: 'Your name',
  checkTexts: 'Check the post texts too: they go out with the video on each service.',
  feedbackPlaceholder: 'What would you change, or like to say?',
  about: 'About',
  wholeEpisode: 'the whole episode',
  atTime: (label, t) => `${label} at ${t}`,
  send: 'Send',
  sent: 'Thanks, your feedback was saved.',
  anonymous: 'Anonymous',
  failed: 'Something went wrong. Please try again.',
}
const lang = navigator.language?.toLowerCase().startsWith('fi') ? 'fi' : 'en'
document.documentElement.lang = lang
const t = (key, ...args) => {
  const value = STRINGS[lang]?.[key] ?? EN[key]
  return typeof value === 'function' ? value(...args) : value
}


const $ = (id) => document.getElementById(id)
const [, , token, , episodeId] = location.pathname.split('/')
const api = `/api/s/${token}`

async function getJSON(url, options) {
  const response = await fetch(url, options)
  if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error || response.statusText)
  return response.json()
}

function formatTime(ms) {
  const s = Math.floor(ms / 1000)
  const h = Math.floor(s / 3600)
  const mm = String(Math.floor(s / 60) % 60).padStart(2, '0')
  const ss = String(s % 60).padStart(2, '0')
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`
}

function formatDate(seconds) {
  return new Date(seconds * 1000).toLocaleDateString(lang === 'fi' ? 'fi-FI' : undefined, {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
  })
}

function remembered(key, value) {
  try {
    if (value === undefined) return localStorage.getItem(key) || ''
    localStorage.setItem(key, value)
  } catch {
    // Private windows: nothing is remembered, nothing breaks.
  }
  return ''
}

// ── Series: the episode list ──────────────────────────────────────────────────

async function showSeries() {
  const series = await getJSON(api)
  document.title = `${series.title} · Videoiden hyväksyntä`
  $('series-title').textContent = series.title
  $('main').replaceChildren(
    h('h2', {}, t('episodes')),
    series.episodes.length
      ? h(
          'ul',
          { class: 'list' },
          series.episodes.map((e) =>
            h(
              'li',
              {},
              h(
                'a',
                { class: 'card', href: `/s/${token}/e/${e.id}` },
                h('strong', {}, e.title),
                h('div', { class: 'muted' }, [formatDate(e.createdAt), t('videos', e.videos), t('notes', e.feedback)].join(' · '))
              )
            )
          )
        )
      : h('p', { class: 'muted' }, t('noEpisodes'))
  )
}

// ── Episode: videos, caption checks, feedback ─────────────────────────────────

const players = new Map() // video id → <video>

// Who is checking: shown with their text edits, approvals and feedback.
const nameField = h('input', {
  class: 'field name',
  placeholder: t('yourName'),
  value: remembered('portal:name'),
  autocomplete: 'name',
  oninput: () => remembered('portal:name', nameField.value.trim()),
})
const who = () => nameField.value.trim()

async function showEpisode() {
  const episode = await getJSON(`${api}/episodes/${episodeId}`)
  document.title = `${episode.title} · ${episode.series}`
  $('series-title').textContent = episode.series
  $('where').textContent = episode.title
  players.clear()

  const form = feedbackForm(episode)
  const videos = episode.videos.map((v) => {
    const player = h('video', { src: v.fileUrl, controls: true, preload: 'metadata', playsInline: true })
    players.set(v.id, player)
    const review = v.captionsUrl
      ? `/review/?${new URLSearchParams({
          video: v.fileUrl,
          captions: v.captionsUrl,
          submit: `${api}/videos/${v.id}/review`,
          back: location.pathname,
        })}`
      : null
    const lastReview = v.reviews.at(-1)
    return h(
      'section',
      { class: 'card video' },
      h('h3', {}, v.label),
      player,
      h(
        'div',
        { class: 'actions' },
        review && h('a', { class: 'button primary', href: review }, t('checkCaptions')),
        h('a', { class: 'button', href: v.downloadUrl }, t('download')),
        h(
          'button',
          {
            type: 'button',
            class: 'link',
            onclick: () => form.aim(v.id, Math.round(player.currentTime * 1000)),
          },
          t('commentHere')
        ),
        lastReview && h('span', { class: 'badge ok' }, t('reviewedBy', lastReview.author))
      ),
      postsEditor(v, {
        save: (platform, text) =>
          getJSON(`${api}/videos/${v.id}/posts/${platform}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ ...text, author: who() }),
          }),
        approve: (platform, approved) =>
          getJSON(`${api}/videos/${v.id}/posts/${platform}/approval`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ approved, author: who() }),
          }),
      })
    )
  })

  $('main').replaceChildren(
    h('p', {}, h('a', { href: `/s/${token}` }, t('back'))),
    h('h2', {}, episode.title),
    h('label', { class: 'who' }, t('yourName'), ' ', nameField),
    h('p', { class: 'muted' }, t('checkTexts')),
    videos.length ? h('div', { class: 'videos' }, videos) : h('p', { class: 'muted' }, t('noVideos')),
    h('h2', {}, t('feedback')),
    notesList(episode),
    form.el
  )
}

function notesList(episode) {
  if (!episode.feedback.length) return h('p', { class: 'muted' }, t('noFeedback'))
  const labels = new Map(episode.videos.map((v) => [v.id, v.label]))
  return h(
    'ul',
    { class: 'notes' },
    episode.feedback.map((f) => {
      const where =
        f.videoId && labels.has(f.videoId)
          ? h(
              'button',
              {
                type: 'button',
                class: 'link',
                onclick: () => {
                  const player = players.get(f.videoId)
                  player.currentTime = (f.atMs || 0) / 1000
                  player.scrollIntoView({ block: 'center', behavior: 'smooth' })
                  player.play()
                },
              },
              t('atTime', labels.get(f.videoId), formatTime(f.atMs || 0))
            )
          : null
      return h(
        'li',
        { class: 'note' },
        h('div', { class: 'muted' }, h('strong', {}, f.author || t('anonymous')), ' · ', formatDate(f.createdAt), where ? ' · ' : '', where),
        h('p', {}, f.text)
      )
    })
  )
}

function feedbackForm(episode) {
  const text = h('textarea', { rows: 4, placeholder: t('feedbackPlaceholder'), required: true })
  const about = h('select', {}, h('option', { value: '' }, t('wholeEpisode')))
  const status = h('span', { class: 'muted' })
  let atMs = null

  const el = h(
    'form',
    {
      class: 'feedback',
      onsubmit: async (e) => {
        e.preventDefault()
        const videoId = about.value ? Number(about.value) : null
        try {
          await getJSON(`${api}/episodes/${episodeId}/feedback`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ text: text.value, author: who(), videoId, atMs: videoId ? atMs : null }),
          })
          await showEpisode()
          $('error').textContent = ''
          document.querySelector('form.feedback .muted').textContent = t('sent')
        } catch {
          status.textContent = t('failed')
        }
      },
    },
    h('div', { class: 'row' }, h('label', { class: 'muted' }, t('about'), ' ', about)),
    text,
    h('div', { class: 'actions' }, h('button', { class: 'primary', type: 'submit' }, t('send')), status)
  )

  return {
    el,
    // Point the form at a moment in a video, from its "comment" button.
    aim(videoId, ms) {
      atMs = ms
      const label = episode.videos.find((v) => v.id === videoId)?.label || ''
      about.querySelector('[data-at]')?.remove()
      about.append(h('option', { value: String(videoId), 'data-at': '1' }, t('atTime', label, formatTime(ms))))
      about.value = String(videoId)
      text.focus()
      text.scrollIntoView({ block: 'center', behavior: 'smooth' })
    },
  }
}

;(episodeId ? showEpisode() : showSeries()).catch((err) => {
  $('error').textContent = err.message || t('failed')
})
