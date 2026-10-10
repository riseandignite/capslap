// The editor's side: series and their secret links, episodes, videos, every
// captions version (to import into PyCapSlap) and all feedback.

import { h } from './dom.js'
import { PLATFORMS } from './platforms.js'
import { postsEditor } from './posts.js'

const $ = (id) => document.getElementById(id)


let token = ''
try {
  token = localStorage.getItem('portal:admin') || ''
} catch {
  // no storage: sign in every time
}
let selected = null

async function api(path, options = {}) {
  const response = await fetch(path, {
    ...options,
    headers: { Authorization: `Bearer ${token}`, ...(options.headers || {}) },
  })
  if (response.status === 401) {
    signOut()
    throw new Error('Wrong admin token.')
  }
  if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error || response.statusText)
  return response
}
const json = async (path, options) => (await api(path, options)).json()
const send = (method, path, body) =>
  json(path, { method, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })

function fail(err) {
  $('error').textContent = err.message
}

function signOut() {
  token = ''
  try {
    localStorage.removeItem('portal:admin')
  } catch {
    // ignore
  }
  $('admin').hidden = true
  $('sign-out').hidden = true
  $('sign-in').hidden = false
}

$('sign-in').addEventListener('submit', async (e) => {
  e.preventDefault()
  token = $('token').value.trim()
  try {
    localStorage.setItem('portal:admin', token)
  } catch {
    // ignore
  }
  start()
})
$('sign-out').addEventListener('click', signOut)

const when = (s) => new Date(s * 1000).toLocaleString()
const mb = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`
const clock = (ms) => `${String(Math.floor(ms / 60000)).padStart(2, '0')}:${String(Math.floor(ms / 1000) % 60).padStart(2, '0')}`

// Files behind the admin token can't be plain links: fetch, then save.
async function save(path, fallbackName) {
  const response = await api(path)
  const disposition = response.headers.get('content-disposition') || ''
  const match = /filename\*=utf-8''([^;]+)|filename="([^"]+)"/i.exec(disposition)
  const name = match ? decodeURIComponent(match[1] || match[2]) : fallbackName
  const a = h('a', { href: URL.createObjectURL(await response.blob()), download: name })
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 2000)
}

async function start() {
  $('sign-in').hidden = true
  $('admin').hidden = false
  $('sign-out').hidden = false
  $('error').textContent = ''
  try {
    await listSeries()
  } catch (err) {
    fail(err)
  }
}

async function listSeries() {
  const all = await json('/api/admin/series')
  $('series-list').replaceChildren(
    ...all.map((s) =>
      h(
        'li',
        {},
        h(
          'a',
          {
            href: '#',
            class: `card${s.id === selected ? ' selected' : ''}`,
            onclick: (e) => {
              e.preventDefault()
              selected = s.id
              listSeries()
              showSeries(s.id).catch(fail)
            },
          },
          h('strong', {}, s.title),
          h('div', { class: 'muted' }, `${s.episodes.length} episodes`)
        )
      )
    )
  )
  if (selected) await showSeries(selected)
}

$('new-series').addEventListener('submit', async (e) => {
  e.preventDefault()
  try {
    const s = await send('POST', '/api/admin/series', { title: $('new-series-title').value })
    $('new-series-title').value = ''
    selected = s.id
    await listSeries()
  } catch (err) {
    fail(err)
  }
})

async function showSeries(id) {
  const s = await json(`/api/admin/series/${id}`)
  const link = `${location.origin}${s.link}`
  const linkField = h('input', { class: 'field', value: link, readOnly: true })
  $('detail').replaceChildren(
    h('h2', {}, s.title),
    h('div', { class: 'muted' }, 'Anyone with this link sees this series:'),
    h(
      'div',
      { class: 'linkbox' },
      linkField,
      h('button', { onclick: () => navigator.clipboard.writeText(link) }, 'Copy'),
      h('a', { class: 'button', href: s.link, target: '_blank', rel: 'noopener' }, 'Open'),
      h(
        'button',
        {
          onclick: async () => {
            if (!confirm('Make a new link? The current one stops working.')) return
            await api(`/api/admin/series/${id}/rotate`, { method: 'POST' })
            showSeries(id).catch(fail)
          },
        },
        'New link'
      )
    ),
    h(
      'form',
      {
        class: 'actions',
        style: 'margin-bottom:16px',
        onsubmit: async (e) => {
          e.preventDefault()
          await send('POST', `/api/admin/series/${id}/episodes`, { title: e.target.title.value })
          showSeries(id).catch(fail)
        },
      },
      h('input', { class: 'field', name: 'title', placeholder: 'New episode', required: true, style: 'max-width:320px' }),
      h('button', { type: 'submit' }, 'Add episode')
    ),
    ...s.episodes.map((e) => episodeBlock(id, e)),
    h(
      'p',
      {},
      h(
        'button',
        {
          class: 'danger',
          onclick: async () => {
            if (!confirm(`Delete "${s.title}" with all its episodes, videos and feedback?`)) return
            await api(`/api/admin/series/${id}`, { method: 'DELETE' })
            selected = null
            $('detail').replaceChildren()
            listSeries()
          },
        },
        'Delete series'
      )
    )
  )
}

function episodeBlock(seriesId, e) {
  const refresh = () => showSeries(seriesId).catch(fail)
  const labels = new Map(e.videos.map((v) => [v.id, v.label]))
  return h(
    'section',
    { class: 'card episode' },
    h('h3', {}, e.title, ' ', h('span', { class: 'muted' }, when(e.createdAt))),
    e.videos.length
      ? h(
          'table',
          { class: 'table' },
          h('tr', {}, h('th', {}, 'Video'), h('th', {}, 'Captions'), h('th', {}, '')),
          e.videos.map((v) =>
            h(
              'tr',
              {},
              h(
                'td',
                {},
                h('strong', {}, v.label),
                h('div', { class: 'muted' }, v.ready ? mb(v.size) : 'not uploaded yet'),
                v.ready && h('button', { class: 'link', onclick: () => save(`${v.fileUrl}?download=1`, 'video.mp4').catch(fail) }, 'Download')
              ),
              h(
                'td',
                {},
                v.captions.length
                  ? h(
                      'ul',
                      { class: 'notes' },
                      v.captions
                        .slice()
                        .reverse()
                        .map((c, i) =>
                          h(
                            'li',
                            {},
                            h(
                              'button',
                              {
                                class: 'link',
                                onclick: () =>
                                  save(`/api/admin/videos/${v.id}/captions?version=${c.id}`, 'captions.capslap.json').catch(fail),
                              },
                              c.source === 'review' ? `Reviewed by ${c.author || 'someone'}` : 'Sent out'
                            ),
                            h('span', { class: 'muted' }, ` ${when(c.createdAt)}${i === 0 ? ' (latest)' : ''}`)
                          )
                        )
                    )
                  : h('span', { class: 'muted' }, 'none')
              ),
              h(
                'td',
                {},
                h(
                  'button',
                  {
                    class: 'link danger',
                    onclick: async () => {
                      if (!confirm(`Delete "${v.label}"?`)) return
                      await api(`/api/admin/videos/${v.id}`, { method: 'DELETE' })
                      refresh()
                    },
                  },
                  'Delete'
                )
              )
            )
          )
        )
      : h('p', { class: 'muted' }, 'No videos. Publish one from PyCapSlap, or upload below.'),
    e.videos.map((v) => postsBlock(v)),
    uploadForm(e.id, refresh),
    h('h3', { style: 'margin-top:12px' }, `Feedback (${e.feedback.length})`),
    e.feedback.length
      ? h(
          'ul',
          { class: 'notes' },
          e.feedback.map((f) =>
            h(
              'li',
              { class: 'note' },
              h(
                'div',
                { class: 'muted' },
                h('strong', {}, f.author || 'Anonymous'),
                ` · ${when(f.createdAt)}`,
                f.videoId ? ` · ${labels.get(f.videoId) || 'video'} at ${clock(f.atMs || 0)}` : '',
                ' ',
                h(
                  'button',
                  {
                    class: 'link danger',
                    onclick: async () => {
                      await api(`/api/admin/feedback/${f.id}`, { method: 'DELETE' })
                      refresh()
                    },
                  },
                  'remove'
                )
              ),
              h('p', {}, f.text)
            )
          )
        )
      : h('p', { class: 'muted' }, 'None yet.'),
    h(
      'button',
      {
        class: 'link danger',
        onclick: async () => {
          if (!confirm(`Delete "${e.title}" with its videos and feedback?`)) return
          await api(`/api/admin/episodes/${e.id}`, { method: 'DELETE' })
          refresh()
        },
      },
      'Delete episode'
    )
  )
}

// Where a video goes out and with what text, one details box per video.
function postsBlock(v) {
  const box = h('details', { class: 'video-posts' })
  const draw = () => {
    const chooser = h(
      'div',
      { class: 'actions' },
      h('span', { class: 'muted' }, 'Goes to:'),
      PLATFORMS.map((p) =>
        h(
          'label',
          {},
          h('input', {
            type: 'checkbox',
            checked: v.platforms.includes(p.id),
            onchange: async (e) => {
              const chosen = e.target.checked ? [...v.platforms, p.id] : v.platforms.filter((x) => x !== p.id)
              try {
                v.platforms = (await send('PUT', `/api/admin/videos/${v.id}/platforms`, { platforms: chosen })).platforms
              } catch (err) {
                fail(err)
              }
              draw()
            },
          }),
          ` ${p.name}`
        )
      )
    )
    const approvedCount = v.platforms.filter((id) => v.posts[id]?.approvedAt).length
    box.replaceChildren(
      h('summary', {}, `Post texts: ${v.label} `, h('span', { class: 'muted' }, `(${approvedCount}/${v.platforms.length} approved)`)),
      chooser,
      postsEditor(v, {
        save: async (platform, text) => {
          const post = await send('PUT', `/api/admin/videos/${v.id}/posts/${platform}`, { ...text, author: 'Editor' })
          v.posts[platform] = post
          return post
        },
        approve: async (platform, approved) => {
          const post = await send('POST', `/api/admin/videos/${v.id}/posts/${platform}/approval`, { approved, author: 'Editor' })
          v.posts[platform] = post
          return post
        },
      })
    )
  }
  draw()
  return box
}

function uploadForm(episodeId, refresh) {
  const status = h('span', { class: 'muted' })
  return h(
    'details',
    {},
    h('summary', { class: 'muted' }, 'Upload a video by hand'),
    h(
      'form',
      {
        class: 'feedback',
        onsubmit: async (e) => {
          e.preventDefault()
          const form = e.target
          const file = form.video.files[0]
          let captions
          try {
            captions = form.captions.files[0] ? JSON.parse(await form.captions.files[0].text()) : undefined
          } catch {
            status.textContent = 'The captions file is not JSON.'
            return
          }
          status.textContent = 'Uploading…'
          try {
            const v = await send('POST', `/api/admin/episodes/${episodeId}/videos`, {
              label: form.label.value,
              filename: file.name,
              captions,
            })
            await api(v.uploadUrl, { method: 'PUT', body: file })
            refresh()
          } catch (err) {
            status.textContent = err.message
          }
        },
      },
      h('input', { name: 'label', placeholder: 'Label, e.g. 9:16 teaser', required: true }),
      h('label', { class: 'muted' }, 'Video (mp4) ', h('input', { name: 'video', type: 'file', accept: 'video/mp4', required: true })),
      h('label', { class: 'muted' }, 'Captions (.capslap.json, optional) ', h('input', { name: 'captions', type: 'file', accept: '.json' })),
      h('div', { class: 'actions' }, h('button', { class: 'primary', type: 'submit' }, 'Upload'), status)
    )
  )
}

if (token) start()
else signOut()
