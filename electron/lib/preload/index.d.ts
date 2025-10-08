import { ElectronAPI } from '@electron-toolkit/preload'
import type api from './api'

// Whisper model types
export type WhisperModel = 'tiny' | 'base' | 'small' | 'medium' | 'large' | 'whisper-1'

export interface ModelInfo {
  name: WhisperModel | 'whisper-1'
  size: string
  downloaded: boolean
}

export interface DownloadModelParams {
  model: WhisperModel
}

export interface DownloadModelResult {
  model: string
  path: string
  size: number
}

declare global {
  interface Window {
    electron: ElectronAPI
    api: typeof api
    rust: {
      openFiles: (filters?: any) => Promise<string[] | null>
      call: (method: string, params: any) => Promise<any>
      onProgress: (cb: (p: any) => void) => () => void
      getFilePath: (file: File) => string | null
      downloadModel: (model: WhisperModel) => Promise<DownloadModelResult>
      checkModelExists: (model: WhisperModel) => Promise<boolean>
    }
  }
}
