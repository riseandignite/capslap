import { spawn } from 'node:child_process'
import readline from 'node:readline'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { randomUUID } from 'node:crypto'

type Progress = { event: 'Progress'; id: string; status: string; progress: number }

export class Sidecar {
  private proc: ReturnType<typeof spawn> | null = null
  private rl: readline.Interface | null = null
  private pending = new Map<string, { resolve: (v: any) => void; reject: (e: any) => void }>()
  private progressCb: ((p: Progress) => void) | null = null
  private writeLock = Promise.resolve()

  constructor() {
    console.log('[SIDECAR] Initializing Rust sidecar...')
    this.start()
  }

  private start() {
    const __dirname = path.dirname(fileURLToPath(import.meta.url))
    const binName = process.platform === 'win32' ? 'core.exe' : 'core'
    const binDev = path.resolve(__dirname, '../../../rust/target/debug', binName)

    console.log('[SIDECAR] Starting Rust binary:', binDev)

    this.proc = spawn(binDev, [], { stdio: ['pipe', 'pipe', 'inherit'] })
    this.rl = readline.createInterface({ input: this.proc.stdout! })

    this.proc.on('error', (err) => {
      console.error('[SIDECAR] Process error:', err)
    })

    this.proc.on('exit', (code, signal) => {
      console.log('[SIDECAR] Process exited with code:', code, 'signal:', signal)
    })

    this.rl.on('line', (line) => {
      try {
        console.log('[SIDECAR] Raw response:', line)
        const msg = JSON.parse(line)

        if (msg.event === 'Progress' && this.progressCb) {
          console.log('[SIDECAR] Progress event:', msg)
          this.progressCb(msg)
          return
        }
        if (msg.result && msg.id) {
          console.log('[SIDECAR] Success response for:', msg.id, msg.result)
          this.pending.get(msg.id)?.resolve(msg.result)
          this.pending.delete(msg.id)
        } else if (msg.error && msg.id) {
          console.log('[SIDECAR] Error response for:', msg.id, msg.error)
          this.pending.get(msg.id)?.reject(new Error(msg.error))
          this.pending.delete(msg.id)
        }
      } catch (err) {
        console.error('[SIDECAR] Failed to parse response:', line, err)
      }
    })

    console.log('[SIDECAR] Rust sidecar started successfully')
  }

  private async writeWithLock(data: string): Promise<void> {
    // Chain this write after the previous one completes
    this.writeLock = this.writeLock
      .then(async () => {
        return new Promise<void>((resolve, reject) => {
          if (!this.proc || !this.proc.stdin) {
            reject(new Error('Sidecar process not available'))
            return
          }

          console.log('[SIDECAR] Writing to process:', data.trim())
          this.proc.stdin.write(data, 'utf8', (err) => {
            if (err) {
              console.error('[SIDECAR] Write error:', err)
              reject(err)
            } else {
              console.log('[SIDECAR] Write successful')
              // Small delay to ensure the write is fully flushed before next write
              setTimeout(resolve, 5)
            }
          })
        })
      })
      .catch((err) => {
        console.error('[SIDECAR] Write lock chain error:', err)
        throw err
      })

    return this.writeLock
  }

  call(method: string, params: any, onProgress?: (p: Progress) => void) {
    const id = randomUUID()
    console.log('[SIDECAR] Calling method:', method, 'with params:', params, 'id:', id)

    if (onProgress) this.progressCb = onProgress
    const req = JSON.stringify({ id, method, params }) + '\n'

    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject })

      console.log('[SIDECAR] Sending request:', req.trim())
      this.writeWithLock(req).catch((err) => {
        console.error('[SIDECAR] Failed to write to process:', err)
        this.pending.delete(id)
        reject(err)
      })
    })
  }
}
