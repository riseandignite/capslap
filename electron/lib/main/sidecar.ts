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
    
    // Try different paths for development vs production
    const possiblePaths = [
      // Production paths (extraResources goes to Resources/ directly) - prioritize these first
      path.resolve(process.resourcesPath || __dirname, binName),
      path.resolve(__dirname, '../../../app.asar.unpacked/resources', binName),
      // Development path
      path.resolve(__dirname, '../../../rust/target/debug', binName),
      // Alternative production paths  
      path.resolve(process.resourcesPath || __dirname, 'app.asar.unpacked/resources', binName),
      path.resolve(__dirname, '../../resources', binName),
      // Bundled in app resources
      path.resolve(__dirname, '../../', binName)
    ]
    
    let binPath = possiblePaths[0] // Default to dev path
    
    // Find the first existing path
    for (const possiblePath of possiblePaths) {
      try {
        if (require('fs').existsSync(possiblePath)) {
          binPath = possiblePath
          break
        }
      } catch (e) {
        // Continue to next path
      }
    }

    console.log('[SIDECAR] Trying binary paths:', possiblePaths)
    console.log('[SIDECAR] Starting Rust binary:', binPath)

    try {
      // Set working directory to the same directory as the binary
      const workingDir = path.dirname(binPath)
      const binDir = path.join(workingDir, 'bin')
      const ffmpegBin = process.platform === 'win32' ? 'ffmpeg.exe' : 'ffmpeg'
      const ffmpegPath = path.join(binDir, ffmpegBin)

      // Ensure PATH includes our bin dir, and pass FFMPEG_PATH to Rust
      const sep = process.platform === 'win32' ? ';' : ':'
      const env = {
        ...process.env,
        PATH: `${binDir}${sep}${process.env.PATH || ''}`,
        FFMPEG_PATH: ffmpegPath,
      }

      this.proc = spawn(binPath, [], {
        stdio: ['pipe', 'pipe', 'inherit'],
        cwd: workingDir,  // Set working directory for proper relative path resolution
        env,
      })
      this.rl = readline.createInterface({ input: this.proc.stdout! })

      this.proc.on('error', (err) => {
        console.error('[SIDECAR] Process error:', err)
        console.error('[SIDECAR] Binary path was:', binPath)
        console.error('[SIDECAR] Possible paths tried:', possiblePaths)
      })
    } catch (err) {
      console.error('[SIDECAR] Failed to spawn process:', err)
      console.error('[SIDECAR] Binary path was:', binPath)
      console.error('[SIDECAR] Possible paths tried:', possiblePaths)
      throw err
    }

    this.proc.on('exit', (code, signal) => {
      console.log('[SIDECAR] Process exited with code:', code, 'signal:', signal)
    })

    this.rl.on('line', (line) => {
      try {
        console.log('[SIDECAR] Raw response:', line)
        const msg = JSON.parse(line)

        if (msg.event === 'progress' && this.progressCb) {
          console.log('[SIDECAR] Progress event:', msg)
          this.progressCb(msg)
          return
        }
        if (msg.result !== undefined && msg.id) {
          console.log('[SIDECAR] Success response for:', msg.id, msg.result)
          this.pending.get(msg.id)?.resolve(msg.result)
          this.pending.delete(msg.id)
        } else if (msg.error && msg.id) {
          console.log('[SIDECAR] Error response for:', msg.id, msg.error)
          const friendlyError = this.createFriendlyError(msg.error)
          this.pending.get(msg.id)?.reject(friendlyError)
          this.pending.delete(msg.id)
        }
      } catch (err) {
        console.error('[SIDECAR] Failed to parse response:', line, err)
      }
    })

    console.log('[SIDECAR] Rust sidecar started successfully')
  }

  private createFriendlyError(errorMessage: string): Error {
    const error = new Error()
    
    // Determine error type and create user-friendly message
    if (errorMessage.includes('API key not provided') || errorMessage.includes('You didn\'t provide an API key')) {
      error.name = 'API_KEY_MISSING'
      error.message = 'OpenAI API key is not configured. Add it in settings for better transcription quality.'
    } else if (errorMessage.includes('401 Unauthorized') || errorMessage.includes('Unauthorized')) {
      error.name = 'API_KEY_INVALID'  
      error.message = 'Invalid OpenAI API key. Please check the key in settings.'
    } else if (errorMessage.includes('No whisper models found') || errorMessage.includes('No local whisper models available')) {
      error.name = 'NO_LOCAL_MODELS'
      error.message = 'Local models not found. Using online transcription via OpenAI API.'
    } else if (errorMessage.includes('whisper.cpp binary not found') || errorMessage.includes('FFmpeg not found')) {
      error.name = 'BINARY_NOT_FOUND'
      error.message = 'System components not found. Try reinstalling the application.'
    } else if (errorMessage.includes('Network') || errorMessage.includes('fetch') || errorMessage.includes('ENOTFOUND')) {
      error.name = 'NETWORK_ERROR'
      error.message = 'Internet connection problem. Check your connection and try again.'
    } else if (errorMessage.includes('rate limit') || errorMessage.includes('Too Many Requests')) {
      error.name = 'RATE_LIMIT'
      error.message = 'OpenAI API rate limit exceeded. Try later or check your plan.'
    } else if (errorMessage.includes('insufficient_quota') || errorMessage.includes('quota')) {
      error.name = 'QUOTA_EXCEEDED'
      error.message = 'OpenAI API quota exhausted. Top up your account or use local models.'
    } else if (errorMessage.includes('file not found') || errorMessage.includes('No such file')) {
      error.name = 'FILE_NOT_FOUND'
      error.message = 'File not found. Make sure the video file exists and is accessible.'
    } else if (errorMessage.includes('dyld: Library not loaded') || errorMessage.includes('image not found')) {
      error.name = 'BINARY_DEP_MISSING'
      error.message = 'Media tools are missing system libraries. Use static ffmpeg/ffprobe builds or reinstall.'
    } else {
      // For unknown errors, keep original message
      error.name = 'UNKNOWN_ERROR'
      error.message = `An error occurred: ${errorMessage}`
    }
    
    return error
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
