import { ModelInfo } from '@/lib/preload'

export interface Template {
  id: 'oneliner' | 'karaoke' | 'vibrant'
  captionStyle: 'karaoke' | 'oneliner' | 'vibrant'
  name: string
  src: string | null
  textColor: string
  highlightWordColor: string
  outlineColor: string
  glowEffect: boolean
  font: string
  position: 'bottom' | 'center'
}

export interface Settings {
  selectedTemplate: Template['id']
  exportFormats: string[]
  selectedFont: string
  selectedModel: ModelInfo['name']
  textColor: string
  highlightWordColor: string
  outlineColor: string
  glowEffect: boolean
  captionStyle: Template['captionStyle']
  captionPosition: 'bottom' | 'center'
  selectedLanguage: string
}

export const defaultSettings: Settings = {
  selectedTemplate: 'karaoke',
  exportFormats: ['9:16'],
  selectedFont: 'montserrat-black',
  selectedModel: 'whisper-1',
  textColor: '#ffffff',
  highlightWordColor: '#ffff00',
  outlineColor: '#000000',
  glowEffect: false,
  captionStyle: 'karaoke',
  captionPosition: 'bottom',
  selectedLanguage: 'en',
}

export const FONT_NAMES = {
  'montserrat-black': 'Montserrat Black',
  'komika-axis': 'Komika Axis',
  theboldfont: 'THEBOLDFONT',
  'kanit-bold': 'Kanit Bold',
  'poppins-black': 'Poppins Black',
  'oswald-bold': 'Oswald Bold',
  'bangers-regular': 'Bangers Regular',
  'worksans-bold': 'WorkSans Bold',
  'roboto-bold': 'Roboto Bold',
} as const

export const getFontName = (fontId: string): string => {
  return FONT_NAMES[fontId as keyof typeof FONT_NAMES] || 'Montserrat Black'
}

export const availableFonts = [
  { id: 'komika-axis', name: 'Komika Axis' },
  { id: 'montserrat-black', name: 'Montserrat Black' },
  { id: 'theboldfont', name: 'THEBOLDFONT' },
  { id: 'kanit-bold', name: 'Kanit Bold' },
  { id: 'poppins-black', name: 'Poppins Black' },
  { id: 'oswald-bold', name: 'Oswald Bold' },
  { id: 'bangers-regular', name: 'Bangers Regular' },
  { id: 'worksans-bold', name: 'WorkSans Bold' },
  { id: 'roboto-bold', name: 'Roboto Bold' },
]

export const availableExportFormats = [
  { id: '9:16', name: '9:16', description: 'Perfect for TikTok, Instagram Stories, YouTube Shorts' },
  { id: '16:9', name: '16:9', description: 'Standard for YouTube, desktop viewing' },
  { id: '1:1', name: '1:1', description: 'Instagram posts, Facebook' },
  { id: '4:5', name: '4:5', description: 'Instagram feed posts' },
]

