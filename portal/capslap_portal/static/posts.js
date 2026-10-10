// The texts a video goes out with, one per service, checked against what each
// service takes. The client page and the admin page both use this editor.

import { h } from './dom.js'
import { checkPost, platform, PLATFORMS, visiblePart } from './platforms.js'

const FI = {
  heading: 'Julkaisutekstit',
  title: 'Otsikko',
  text: 'Teksti',
  copy: 'Kopioi',
  copied: 'Kopioitu',
  saving: 'Tallennetaan…',
  saved: 'Tallennettu',
  saveFailed: 'Tallennus epäonnistui',
  approve: 'Hyväksyn tämän tekstin',
  approvedBy: (who, when) => `Hyväksynyt ${who || 'nimetön'} ${when}`,
  editedBy: (who, when) => `Muokannut ${who || 'nimetön'} ${when}`,
  notYet: 'Ei vielä tekstiä.',
  startFrom: 'Aloita tekstistä:',
  visible: 'Näkyy ennen ”lisää”:',
  hashtags: (n) => `${n} aihetunniste${n === 1 ? '' : 'tta'}`,
  noTitle: 'Otsikko puuttuu.',
  noBody: 'Teksti puuttuu.',
  titleTooLong: ({ max, over }) => `Otsikko on ${over} merkkiä liian pitkä (enintään ${max}).`,
  bodyTooLong: ({ max, over }) => `Teksti on ${over} merkkiä liian pitkä (enintään ${max}).`,
  overApiMax: ({ max }) => `Ajastustyökalut hyväksyvät vain ${max} merkkiä; sovelluksessa julkaistessa pidempi käy.`,
  hashtagsIgnored: ({ max, count }) => `${count} aihetunnistetta: vain ${max} ensimmäistä huomioidaan.`,
  hashtagsAllIgnored: ({ max, count }) => `${count} aihetunnistetta: yli ${max}, joten kaikki ohitetaan.`,
  forbiddenChars: 'Merkit < ja > eivät käy.',
}
const EN = {
  heading: 'Post texts',
  title: 'Title',
  text: 'Text',
  copy: 'Copy',
  copied: 'Copied',
  saving: 'Saving…',
  saved: 'Saved',
  saveFailed: 'Could not save',
  approve: 'I approve this text',
  approvedBy: (who, when) => `Approved by ${who || 'someone'} ${when}`,
  editedBy: (who, when) => `Edited by ${who || 'someone'} ${when}`,
  notYet: 'No text yet.',
  startFrom: 'Start from:',
  visible: 'Shown before “more”:',
  hashtags: (n) => `${n} hashtag${n === 1 ? '' : 's'}`,
  noTitle: 'No title.',
  noBody: 'No text.',
  titleTooLong: ({ max, over }) => `The title is ${over} characters too long (max ${max}).`,
  bodyTooLong: ({ max, over }) => `The text is ${over} characters too long (max ${max}).`,
  overApiMax: ({ max }) => `Scheduling tools take only ${max} characters; posting in the app allows more.`,
  hashtagsIgnored: ({ max, count }) => `${count} hashtags: only the first ${max} count.`,
  hashtagsAllIgnored: ({ max, count }) => `${count} hashtags: over ${max}, so all are ignored.`,
  forbiddenChars: 'The characters < and > are not allowed.',
}
const lang = navigator.language?.toLowerCase().startsWith('fi') ? 'fi' : 'en'
const S = lang === 'fi' ? FI : EN
const say = (key, ...args) => (typeof S[key] === 'function' ? S[key](...args) : S[key])
const approved = (post) => post?.approvedAt !== null && post?.approvedAt !== undefined
const when = (s) => new Date(s * 1000).toLocaleString(lang === 'fi' ? 'fi-FI' : undefined, { dateStyle: 'short', timeStyle: 'short' })

/**
 * The editor for one video. `api` does the saving:
 *   save(platformId, { title, body }) → the stored post
 *   approve(platformId, approved)     → the stored post
 */
export function postsEditor(video, api) {
  const posts = { ...video.posts }
  const tabs = h('div', { class: 'post-tabs', role: 'tablist' })
  const panel = h('div', { class: 'post-panel' })
  let current = video.platforms[0]

  function tabState(id) {
    const post = posts[id]
    if (!post?.body && !post?.title) return 'empty'
    if (checkPost(platform(id), post).problems.some((p) => p.level === 'error')) return 'error'
    return approved(post) ? 'approved' : 'draft'
  }

  function drawTabs() {
    tabs.replaceChildren(
      ...video.platforms.map((id) =>
        h(
          'button',
          {
            type: 'button',
            role: 'tab',
            class: `post-tab ${tabState(id)}${id === current ? ' current' : ''}`,
            'aria-selected': String(id === current),
            onclick: () => {
              current = id
              drawTabs()
              drawPanel()
            },
          },
          platform(id).name
        )
      )
    )
  }

  function drawPanel() {
    const p = platform(current)
    const post = posts[current] || { title: '', body: '' }
    const status = h('span', { class: 'muted' })
    const problemList = h('ul', { class: 'problems' })
    const counter = h('span', { class: 'counter' })
    const titleCounter = h('span', { class: 'counter' })
    const visible = h('div', { class: 'visible-part' })
    const title = p.title && h('input', { class: 'field', value: post.title || '', placeholder: say('title') })
    const body = h('textarea', { class: 'field', rows: 6, value: post.body || '', placeholder: say('text') })
    const approve = h('input', { type: 'checkbox', checked: approved(post) })
    const meta = h('div', { class: 'muted' })

    function refresh() {
      const text = { title: title ? title.value : '', body: body.value }
      const check = checkPost(p, text)
      counter.textContent = `${check.bodyLength} / ${p.body.max}${check.hashtags.length ? ` · ${say('hashtags', check.hashtags.length)}` : ''}`
      counter.classList.toggle('over', check.bodyLength > p.body.max)
      if (title) {
        titleCounter.textContent = `${check.titleLength} / ${p.title.max}`
        titleCounter.classList.toggle('over', check.titleLength > p.title.max)
      }
      problemList.replaceChildren(...check.problems.map((pr) => h('li', { class: pr.level }, say(pr.code, pr))))
      const shown = visiblePart(p, text.body)
      visible.replaceChildren(...(shown ? [h('span', { class: 'muted' }, say('visible'), ' '), shown, '…'] : []))
      const saved = posts[current]
      meta.replaceChildren(
        approved(saved)
          ? say('approvedBy', saved.approvedBy, when(saved.approvedAt))
          : saved?.updatedAt
            ? say('editedBy', saved.updatedBy, when(saved.updatedAt))
            : ''
      )
      approve.checked = approved(saved)
      if (startFrom) startFrom.hidden = Boolean(text.body)
      approve.disabled = !saved || (!saved.body && !saved.title)
    }

    let timer = null
    function scheduleSave() {
      refresh()
      clearTimeout(timer)
      status.textContent = say('saving')
      timer = setTimeout(async () => {
        try {
          posts[current] = await api.save(p.id, { title: title ? title.value : '', body: body.value })
          status.textContent = say('saved')
        } catch {
          status.textContent = say('saveFailed')
        }
        refresh()
        drawTabs()
      }, 700)
    }
    title?.addEventListener('input', scheduleSave)
    body.addEventListener('input', scheduleSave)
    approve.addEventListener('change', async () => {
      try {
        posts[current] = await api.approve(p.id, approve.checked)
      } catch {
        status.textContent = say('saveFailed')
      }
      refresh()
      drawTabs()
    })

    const copyButton = (field) =>
      h(
        'button',
        {
          type: 'button',
          onclick: async (e) => {
            await navigator.clipboard.writeText(field.value)
            e.target.textContent = say('copied')
            setTimeout(() => (e.target.textContent = say('copy')), 1500)
          },
        },
        say('copy')
      )

    // A new service's text usually starts from one already written.
    const others = PLATFORMS.filter((o) => o.id !== p.id && posts[o.id]?.body)
    const startFrom =
      !post.body && others.length
        ? h(
            'div',
            { class: 'actions' },
            h('span', { class: 'muted' }, say('startFrom')),
            others.map((o) =>
              h(
                'button',
                {
                  type: 'button',
                  class: 'link',
                  onclick: () => {
                    body.value = posts[o.id].body
                    if (title && !title.value) title.value = posts[o.id].title || ''
                    scheduleSave()
                  },
                },
                o.name
              )
            )
          )
        : null

    panel.replaceChildren(
      ...[
      title && h('label', { class: 'post-field' }, h('span', { class: 'row' }, h('strong', {}, say('title')), titleCounter, copyButton(title)), title),
      h('label', { class: 'post-field' }, h('span', { class: 'row' }, h('strong', {}, say('text')), counter, copyButton(body)), body),
      startFrom,
      visible,
      problemList,
      h('div', { class: 'actions' }, h('label', {}, approve, ' ', say('approve')), status),
      meta,
      ].filter(Boolean)
    )
    refresh()
  }

  const el = h('div', { class: 'posts' }, h('h4', {}, say('heading')), tabs, panel)
  if (!video.platforms.length) return el
  drawTabs()
  drawPanel()
  return el
}
