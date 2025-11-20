import { Input } from './ui/input'

interface ColorPickerProps {
  label: string
  value: string
  onChange: (value: string) => void
}

export function ColorPicker({ label, value, onChange }: ColorPickerProps) {
  return (
    <div className="space-y-3">
      <label className="text-xs text-muted-foreground mb-1 block">{label}</label>
      <div className="flex items-center gap-3">
        <div className="relative">
          <input
            type="color"
            value={value}
            onChange={(e) => onChange(e.target.value)}
            className="w-12 h-8 rounded-sm border border-border bg-transparent cursor-pointer overflow-hidden opacity-0 absolute inset-0"
          />
          <div
            className="w-12 h-8 rounded-sm border border-border cursor-pointer transition-all duration-200 hover:scale-105 hover:border-primary/50"
            style={{ backgroundColor: value }}
          />
        </div>
        <div className="flex-1">
          <Input
            type="text"
            value={value}
            onChange={(e) => {
              const val = e.target.value
              if (/^#[0-9A-Fa-f]{0,6}$/.test(val) || val === '') {
                onChange(val)
              }
            }}
            onBlur={(e) => {
              let val = e.target.value
              if (val && !val.startsWith('#')) val = '#' + val
              if (/^#[0-9A-Fa-f]{6}$/.test(val)) {
                onChange(val)
              } else if (val.length > 0) {
                onChange(value)
              }
            }}
            className="w-full h-8 px-2 py-1.5 border border-border rounded-sm bg-background/50 focus:outline-none hover:border-primary/50 focus:border-primary/70 ring-0 focus-visible:ring-0 focus:ring-0 text-foreground transition-all duration-200"
            placeholder="#ffffff"
            maxLength={7}
          />
        </div>
      </div>
    </div>
  )
}

