import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import { deriveKey, isLocked, lockText, unlock, WrongPassword } from './lock.js'

const PASSWORD = 'k7mq-x2fp-9tza-hw4c'

// Locked by the desktop app (app/models/review.py lock_file); both sides read it.
const fromApp = async () => JSON.parse(await readFile(new URL('./testdata/locked.capslap.json', import.meta.url), 'utf8'))

test('opens a file the desktop app locked', async () => {
  const locked = await fromApp()
  assert.ok(isLocked(locked))
  const { text } = await unlock(locked, PASSWORD)
  const data = JSON.parse(text)
  assert.equal(data.segments[0].text, 'Kylläpä on sää')
  assert.equal(data.video.name, 'talk.mp4')
})

test('a wrong password is WrongPassword', async () => {
  await assert.rejects(unlock(await fromApp(), 'guess'), WrongPassword)
})

test('the reply is locked with the same password, and reads back', async () => {
  const { lock } = await unlock(await fromApp(), PASSWORD)
  const reply = await lockText('{"segments":[]}', lock)
  assert.ok(isLocked(reply))
  assert.ok(!reply.data.includes('segments'))
  assert.equal((await unlock(reply, PASSWORD)).text, '{"segments":[]}')
  // The kept key opens it without the password, as the page does for drafts.
  assert.equal((await unlock(reply, '', lock)).text, '{"segments":[]}')
})

test('every lock gets its own iv', async () => {
  const lock = await deriveKey(PASSWORD, new Uint8Array(16), 1000)
  const a = await lockText('same', lock)
  const b = await lockText('same', lock)
  assert.notEqual(a.iv, b.iv)
  assert.notEqual(a.data, b.data)
})

test('plain captions are not locked', () => {
  assert.ok(!isLocked({ segments: [] }))
  assert.ok(!isLocked(null))
})
