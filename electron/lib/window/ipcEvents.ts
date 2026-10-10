import { BrowserWindow, ipcMain, shell } from 'electron'
import os from 'os'

const handleIPC = (channel: string, handler: (...args: any[]) => void) => {
  ipcMain.handle(channel, handler)
}

const winFromEvent = (evt: Electron.IpcMainInvokeEvent): BrowserWindow | undefined => {
  const win = BrowserWindow.fromWebContents(evt.sender)
  return win && !win.isDestroyed() ? win : undefined
}

export const registerWindowIPC = (initialWindow?: BrowserWindow) => {
  if (initialWindow) {
    // Hide the menu bar on the initially created window
    initialWindow.setMenuBarVisibility(false)
  }

  // Register window IPC. Handlers resolve the live window from the event
  // sender so they survive window recreation on macOS (all windows closed
  // and the dock icon is clicked again).
  handleIPC('init-window', (evt) => {
    const mainWindow = winFromEvent(evt)
    const { width, height } = mainWindow?.getBounds() ?? { width: 1200, height: 900 }
    const minimizable = mainWindow?.isMinimizable() ?? true
    const maximizable = mainWindow?.isMaximizable() ?? true
    const platform = os.platform()

    return { width, height, minimizable, maximizable, platform }
  })

  handleIPC('is-window-minimizable', (evt) => winFromEvent(evt)?.isMinimizable() ?? true)
  handleIPC('is-window-maximizable', (evt) => winFromEvent(evt)?.isMaximizable() ?? true)
  handleIPC('window-minimize', (evt) => winFromEvent(evt)?.minimize())
  handleIPC('window-maximize', (evt) => winFromEvent(evt)?.maximize())
  handleIPC('window-close', (evt) => winFromEvent(evt)?.close())
  handleIPC('window-maximize-toggle', (evt) => {
    const mainWindow = winFromEvent(evt)
    if (!mainWindow) return
    if (mainWindow.isMaximized()) {
      mainWindow.unmaximize()
    } else {
      mainWindow.maximize()
    }
  })

  handleIPC('web-undo', (evt) => evt.sender.undo())
  handleIPC('web-redo', (evt) => evt.sender.redo())
  handleIPC('web-cut', (evt) => evt.sender.cut())
  handleIPC('web-copy', (evt) => evt.sender.copy())
  handleIPC('web-paste', (evt) => evt.sender.paste())
  handleIPC('web-delete', (evt) => evt.sender.delete())
  handleIPC('web-select-all', (evt) => evt.sender.selectAll())
  handleIPC('web-reload', (evt) => evt.sender.reload())
  handleIPC('web-force-reload', (evt) => evt.sender.reloadIgnoringCache())
  handleIPC('web-toggle-devtools', (evt) => evt.sender.toggleDevTools())
  handleIPC('web-actual-size', (evt) => evt.sender.setZoomLevel(0))
  handleIPC('web-zoom-in', (evt) => evt.sender.setZoomLevel(evt.sender.zoomLevel + 0.5))
  handleIPC('web-zoom-out', (evt) => evt.sender.setZoomLevel(evt.sender.zoomLevel - 0.5))
  handleIPC('web-toggle-fullscreen', (evt) => {
    const mainWindow = winFromEvent(evt)
    if (mainWindow) mainWindow.setFullScreen(!mainWindow.fullScreen)
  })
  handleIPC('web-open-url', (_evt, url) => shell.openExternal(url))
}
