import { ElectronAPI } from '@electron-toolkit/preload'
import type api from './api'

declare global {
  interface Window {
    electron: ElectronAPI
    api: typeof api
    rust: {
      openFiles: (filters?: any) => Promise<string[] | null>
      call: (method: string, params: any) => Promise<any>
      onProgress: (cb: (p: any) => void) => () => void
      getFilePath: (file: File) => string | null
    }
  }
}
