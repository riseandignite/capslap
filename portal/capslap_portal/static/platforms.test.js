import assert from 'node:assert/strict'
import { test } from 'node:test'
import { checkPost, hashtags, platform, visiblePart, xLength } from './platforms.js'

test('X counts links as 23 and emoji and CJK as 2, Finnish letters as 1', () => {
  assert.equal(xLength('Hyvää yötä'), 10)
  assert.equal(xLength('katso https://peliteoria.fi/jakso/4?utm=x'), 6 + 23)
  assert.equal(xLength('🎧'), 2)
  assert.equal(xLength('日本'), 4)
})

test('hashtags are found at word starts, not inside words or links', () => {
  assert.deepEqual(hashtags('#tekoäly ja #AI, mutta ei a#b. (#pelit)'), ['tekoäly', 'AI', 'pelit'])
})

test('a post over the limit is an error, with how much it is over', () => {
  const { problems, bodyLength } = checkPost(platform('x'), { body: 'a'.repeat(279) + '🎧' })
  assert.equal(bodyLength, 281)
  assert.deepEqual(problems, [{ level: 'error', code: 'bodyTooLong', max: 280, over: 1 }])
  assert.deepEqual(checkPost(platform('x'), { body: 'a'.repeat(280) }).problems, [])
})

test('instagram ignores hashtags past five; youtube ignores all past fifteen', () => {
  const six = '#a #b #c #d #e #f'
  assert.equal(checkPost(platform('instagram'), { body: six }).problems[0].code, 'hashtagsIgnored')
  const sixteen = Array.from({ length: 16 }, (_, i) => `#t${i}`).join(' ')
  const yt = checkPost(platform('youtube'), { title: 'Otsikko', body: sixteen }).problems
  assert.equal(yt[0].code, 'hashtagsAllIgnored')
})

test('youtube wants a title of at most 100 and no angle brackets', () => {
  const codes = (post) => checkPost(platform('youtube'), post).problems.map((p) => p.code)
  assert.deepEqual(codes({ title: '', body: 'x' }), ['noTitle'])
  assert.deepEqual(codes({ title: 'x'.repeat(101), body: 'x' }), ['titleTooLong'])
  assert.deepEqual(codes({ title: 'a <b>', body: 'x' }), ['forbiddenChars'])
})

test('tiktok past the API limit is a warning, not an error', () => {
  assert.deepEqual(checkPost(platform('tiktok'), { body: 'x'.repeat(2500) }).problems, [
    { level: 'warn', code: 'overApiMax', max: 2200 },
  ])
})

test('the part shown before "more"', () => {
  assert.equal(visiblePart(platform('instagram'), 'short'), null)
  assert.equal(visiblePart(platform('instagram'), 'x'.repeat(130)).length, 125)
  assert.equal(visiblePart(platform('x'), 'x'.repeat(300)), null)
})
