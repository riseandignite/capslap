import React from 'react'

export function TitleBar() {
  const maximize = () => window.electron?.ipcRenderer.invoke('window-maximize-toggle')

  return (
    <div
      className="fixed top-0 left-0 right-0 h-9 bg-black/90 flex items-center justify-between px-3 z-50 drag-region border-b border-white/10"
      onDoubleClick={maximize}
    ></div>
  )
}
