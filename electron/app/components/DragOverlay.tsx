import { Upload } from 'lucide-react'

export function DragOverlay() {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
      <div className="absolute inset-0 bg-[#08090a] backdrop-blur-md animate-in fade-in duration-300" />

      <div className="relative z-10 flex flex-col items-center justify-center p-12 rounded-3xl border-2 border-dashed border-primary/70 bg-gradient-to-br from-primary/20 via-primary/10 to-primary/20 backdrop-blur-xl animate-in zoom-in-95 fade-in duration-300 shadow-2xl">
        <div className="relative mb-8">
          <div className="absolute inset-0 rounded-full bg-primary/20 blur-xl animate-pulse" />
          <div className="relative p-4 rounded-full bg-primary/20 border border-primary/30">
            <Upload className="w-12 h-12 text-primary" />
          </div>
        </div>

        <div className="text-center space-y-3">
          <h3 className="text-2xl font-medium text-primary">Drop your video to upload</h3>
          <div className="flex items-center justify-center gap-2 text-primary/70 text-sm">
            <span>Supported:</span>
            <div className="flex gap-1">
              {['MP4', 'MOV', 'MKV', 'AVI'].map((format) => (
                <span key={format} className="px-1.5 py-0.5 bg-primary/20 rounded text-xs font-mono">
                  {format}
                </span>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

