// The services videos are posted to, and what each one takes in a post.
// Limits as of 2026-09; the services change them, so check here first when one
// complains. The server knows only the ids (app.py PLATFORMS), in this order.

export const PLATFORMS = [
  {
    id: 'youtube',
    name: 'YouTube',
    title: { max: 100 },
    body: { max: 5000 },
    // Over 15 hashtags and YouTube ignores all of them.
    hashtags: { max: 15, all: true },
    forbidden: /[<>]/,
  },
  // 4000 in the app; scheduling tools that post through the API get 2200.
  { id: 'tiktok', name: 'TikTok', body: { max: 4000, apiMax: 2200 } },
  // Since 2025-12 only five hashtags count; the rest are ignored.
  { id: 'instagram', name: 'Instagram', body: { max: 2200, fold: 125 }, hashtags: { max: 5 } },
  { id: 'facebook', name: 'Facebook', body: { max: 63206, fold: 125 } },
  { id: 'linkedin', name: 'LinkedIn', body: { max: 3000, fold: 210 } },
  // Free accounts; links count as 23, most non-Latin characters and emoji as 2.
  { id: 'x', name: 'X', body: { max: 280, weighted: true } },
]

export const platform = (id) => PLATFORMS.find((p) => p.id === id)

const URL_RE = /https?:\/\/[^\s]+/g
const HASHTAG_RE = /(^|[^\p{L}\p{N}_])#([\p{L}\p{N}_]+)/gu

// X's own counting (twitter-text v3): these code points weigh 1, the rest 2.
const LIGHT = [
  [0, 4351],
  [8192, 8205],
  [8208, 8223],
  [8242, 8247],
]

/** Length of `text` the way X counts it against 280. */
export function xLength(text) {
  let length = 0
  const rest = text.normalize('NFC').replace(URL_RE, () => {
    length += 23
    return ''
  })
  for (const ch of rest) {
    const cp = ch.codePointAt(0)
    length += LIGHT.some(([a, b]) => cp >= a && cp <= b) ? 1 : 2
  }
  return length
}

export function hashtags(text) {
  return [...text.matchAll(HASHTAG_RE)].map((m) => m[2])
}

/** Characters as a person counts them: emoji and accented letters are one each. */
export function plainLength(text) {
  return [...text.normalize('NFC')].length
}

/**
 * What's wrong with a post for `p`. Each problem is { level: 'error' | 'warn', code, ...values };
 * errors stop it from going out as written, warnings are worth a look.
 */
export function checkPost(p, { title = '', body = '' }) {
  const problems = []
  const bodyLength = p.body.weighted ? xLength(body) : plainLength(body)
  const titleLength = plainLength(title)
  const tags = hashtags(body)
  if (p.title) {
    if (!title.trim()) problems.push({ level: 'warn', code: 'noTitle' })
    if (titleLength > p.title.max) problems.push({ level: 'error', code: 'titleTooLong', max: p.title.max, over: titleLength - p.title.max })
  }
  if (!body.trim()) problems.push({ level: 'warn', code: 'noBody' })
  if (bodyLength > p.body.max) problems.push({ level: 'error', code: 'bodyTooLong', max: p.body.max, over: bodyLength - p.body.max })
  else if (p.body.apiMax && bodyLength > p.body.apiMax) problems.push({ level: 'warn', code: 'overApiMax', max: p.body.apiMax })
  if (p.hashtags && tags.length > p.hashtags.max) {
    problems.push({ level: 'error', code: p.hashtags.all ? 'hashtagsAllIgnored' : 'hashtagsIgnored', max: p.hashtags.max, count: tags.length })
  }
  if (p.forbidden && (p.forbidden.test(title) || p.forbidden.test(body))) problems.push({ level: 'error', code: 'forbiddenChars' })
  return { titleLength, bodyLength, hashtags: tags, problems }
}

/** The part of the text shown before the service's "more". */
export function visiblePart(p, body) {
  if (!p.body.fold) return null
  const chars = [...body]
  return chars.length > p.body.fold ? chars.slice(0, p.body.fold).join('') : null
}
