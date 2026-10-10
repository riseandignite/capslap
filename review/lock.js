// Password-locked review files. A locked file hides the whole captions file:
//   { format: 'capslap-locked', formatVersion: 1, kdf: 'PBKDF2-SHA256',
//     iterations, salt, iv, data }   (base64; data is AES-256-GCM of the JSON)
// The desktop app reads and writes the same format (app/models/review.py).

export const LOCKED_FORMAT = 'capslap-locked'
export const ITERATIONS = 600000

const subtle = globalThis.crypto.subtle

export class WrongPassword extends Error {
  constructor() {
    super('Wrong password.')
  }
}

export function isLocked(data) {
  return Boolean(data) && data.format === LOCKED_FORMAT
}

function toBase64(bytes) {
  let binary = ''
  for (const b of bytes) binary += String.fromCharCode(b)
  return btoa(binary)
}

function fromBase64(text) {
  return Uint8Array.from(atob(text), (c) => c.charCodeAt(0))
}

/** The key for `password`; keep it to lock the reply without asking again. */
export async function deriveKey(password, salt, iterations = ITERATIONS) {
  const material = await subtle.importKey('raw', new TextEncoder().encode(password), 'PBKDF2', false, ['deriveKey'])
  const key = await subtle.deriveKey(
    { name: 'PBKDF2', hash: 'SHA-256', salt, iterations },
    material,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt']
  )
  return { key, salt, iterations }
}

/**
 * Opens a locked file: returns its text and the key it was locked with. A `known`
 * key from an earlier unlock is used as is when the file was locked with it.
 */
export async function unlock(locked, password, known = null) {
  const salt = fromBase64(locked.salt)
  const iterations = Number(locked.iterations)
  const reuse = known && known.iterations === iterations && toBase64(known.salt) === locked.salt
  const lock = reuse ? known : await deriveKey(password, salt, iterations)
  try {
    const plain = await subtle.decrypt({ name: 'AES-GCM', iv: fromBase64(locked.iv) }, lock.key, fromBase64(locked.data))
    return { text: new TextDecoder().decode(plain), lock }
  } catch {
    throw new WrongPassword()
  }
}

/** Locks `text` with a key from deriveKey or unlock. */
export async function lockText(text, { key, salt, iterations }) {
  const iv = globalThis.crypto.getRandomValues(new Uint8Array(12))
  const data = await subtle.encrypt({ name: 'AES-GCM', iv }, key, new TextEncoder().encode(text))
  return {
    format: LOCKED_FORMAT,
    formatVersion: 1,
    kdf: 'PBKDF2-SHA256',
    iterations,
    salt: toBase64(salt),
    iv: toBase64(iv),
    data: toBase64(new Uint8Array(data)),
  }
}
