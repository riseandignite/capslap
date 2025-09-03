import { BrowserWindow, shell, app, protocol, net, ipcMain, dialog } from 'electron'
import { join } from 'path'
import { registerWindowIPC } from '@/lib/window/ipcEvents'
import appIcon from '@/resources/build/icon.png?asset'
import { pathToFileURL } from 'url'
import { Sidecar } from './sidecar'

let core: Sidecar | null = null
let ipcRegistered = false
let protocolRegistered = false

export function createAppWindow(): void {
  // Register custom protocol for resources only once
  if (!protocolRegistered) {
    registerResourcesProtocol()
    protocolRegistered = true
  }

  // Initialize Rust sidecar
  if (!core) {
    core = new Sidecar()
  }

  // Create the main window.
  const mainWindow = new BrowserWindow({
    width: 1200,
    height: 900,
    minWidth: 1200,
    minHeight: 800,
    show: false,
    backgroundColor: '#0a0a0a',
    icon: appIcon,
    frame: false,
    titleBarStyle: 'hiddenInset',
    title: 'CapSlap',
    maximizable: true,
    resizable: true,
    webPreferences: {
      preload: join(__dirname, '../preload/preload.js'),
      sandbox: false,
      contextIsolation: true,
      nodeIntegration: false,
    },
  })

  // Register IPC events only once
  if (!ipcRegistered) {
    registerWindowIPC(mainWindow)
    registerRustIPC(mainWindow)
    ipcRegistered = true
  }

  mainWindow.on('ready-to-show', () => {
    mainWindow.show()
  })

  mainWindow.webContents.setWindowOpenHandler((details) => {
    shell.openExternal(details.url)
    return { action: 'deny' }
  })

  // HMR for renderer base on electron-vite cli.
  // Load the remote URL for development or the local html file for production.
  if (!app.isPackaged && process.env['ELECTRON_RENDERER_URL']) {
    mainWindow.loadURL(process.env['ELECTRON_RENDERER_URL'])
  } else {
    mainWindow.loadFile(join(__dirname, '../renderer/index.html'))
  }
}

// Register custom protocol for assets
function registerResourcesProtocol() {
  if (!protocol.isProtocolHandled('res')) {
    protocol.handle('res', async (request) => {
      try {
        const url = new URL(request.url)
        const relativePath = url.href.replace('res://', '')

        const possiblePaths = [
          join(__dirname, '../../resources', relativePath),
          join(__dirname, '../../../resources', relativePath),
          join(process.resourcesPath, relativePath),
          join(process.resourcesPath, 'app.asar.unpacked', 'resources', relativePath),
        ]

        let filePath: string | null = null
        for (const path of possiblePaths) {
          if (require('fs').existsSync(path)) {
            filePath = path
            break
          }
        }

        if (!filePath) {
          console.error('File not found in any location')
          return new Response('Resource not found', { status: 404 })
        }

        const response = await net.fetch(pathToFileURL(filePath).toString())

        if (relativePath.endsWith('.mp4')) {
          const buffer = await response.arrayBuffer()

          return new Response(buffer, {
            status: 200,
            headers: {
              'Content-Type': 'video/mp4',
              'Accept-Ranges': 'bytes',
              'Access-Control-Allow-Origin': '*',
              'Content-Length': buffer.byteLength.toString(),
            },
          })
        }

        return response
      } catch (error) {
        console.error('Protocol error:', error)
        return new Response('Resource not found', { status: 404 })
      }
    })
  }
}

function registerRustIPC(mainWindow: BrowserWindow) {
  ipcMain.handle('dialog:openFiles', async (_evt, payload) => {
    console.log('[MAIN] File dialog requested:', payload)
    const props: any[] = ['openFile', 'multiSelections']
    const filters = payload?.filters ?? undefined
    const res = await dialog.showOpenDialog(mainWindow, { properties: props, filters })
    if (res.canceled || res.filePaths.length === 0) {
      console.log('[MAIN] File dialog cancelled')
      return null
    }
    console.log('[MAIN] File selected:', res.filePaths)
    return res.filePaths
  })

  ipcMain.handle('core:call', async (_evt, payload) => {
    console.log('[MAIN] Core call:', payload.method, payload.params)
    if (!core) {
      console.error('[MAIN] Core sidecar not initialized')
      throw new Error('Core sidecar not initialized')
    }
    return core.call(payload.method, payload.params, (p) => {
      console.log('[MAIN] Core progress:', p)
      mainWindow.webContents.send('core:progress', p)
    })
  })
}
