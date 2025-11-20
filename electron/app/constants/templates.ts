import { Template } from '../types'
import { getVideoPath } from '../utils/video'

export const templates: Template[] = [
  {
    id: 'oneliner',
    captionStyle: 'oneliner',
    name: 'Oneliner',
    src: getVideoPath('oneliner.mp4'),
    textColor: '#ffffff',
    highlightWordColor: '#ffff00',
    outlineColor: '#000000',
    glowEffect: true,
    font: 'montserrat-black',
    position: 'bottom',
  },
  {
    id: 'karaoke',
    captionStyle: 'karaoke',
    name: 'Karaoke',
    src: getVideoPath('karaoke.mp4'),
    textColor: '#ffffff',
    highlightWordColor: '#00f924',
    outlineColor: '#000000',
    glowEffect: false,
    font: 'komika-axis',
    position: 'bottom',
  },
  {
    id: 'vibrant',
    captionStyle: 'vibrant',
    name: 'Vibrant',
    src: getVideoPath('vibrant.mp4'),
    textColor: '#898284',
    highlightWordColor: '#7ef1c5',
    outlineColor: '#000000',
    glowEffect: false,
    font: 'roboto-bold',
    position: 'center',
  },
]

