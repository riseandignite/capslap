import { toast } from 'sonner'

export const showErrorToast = (error: any, count: number = 1) => {
  const countText = count > 1 ? ` (${count} videos)` : ''

  switch (error.name) {
    case 'API_KEY_MISSING':
      toast.error('🔑 API Key Not Configured', {
        description: 'Add OpenAI API key in settings for better transcription quality.',
        action: {
          label: 'Settings',
          onClick: () => {
            // Will open API key settings
          },
        },
      })
      break

    case 'API_KEY_INVALID':
      toast.error('🔑 Invalid API Key', {
        description: 'Please check your OpenAI API key in settings.',
      })
      break

    case 'NO_LOCAL_MODELS':
      toast.warning('📥 Using Online Transcription', {
        description: 'Local models not found. Transcription performed via OpenAI API.',
      })
      break

    case 'BINARY_NOT_FOUND':
      toast.error('⚙️ System Error', {
        description: 'Application components not found. Try reinstalling CapSlap.',
      })
      break

    case 'BINARY_DEP_MISSING':
      toast.error('🧩 Missing System Libraries', {
        description: 'Media tools require static ffmpeg/ffprobe builds. Please reinstall or contact support.',
      })
      break

    case 'NETWORK_ERROR':
      toast.error('🌐 Internet Connection Problem', {
        description: 'Check your network connection and try again.',
      })
      break

    case 'RATE_LIMIT':
      toast.error('⏰ Rate Limit Exceeded', {
        description: 'Too many requests to OpenAI API. Try again later.',
      })
      break

    case 'QUOTA_EXCEEDED':
      toast.error('💳 API Quota Exhausted', {
        description: 'Top up your OpenAI balance or use local models.',
      })
      break

    case 'FILE_NOT_FOUND':
      toast.error('📁 File Not Found', {
        description: 'Make sure the video file exists and is accessible.',
      })
      break

    default:
      toast.error(`❌ Error${countText}`, {
        description: error.message || 'An unexpected error occurred. Please try again.',
      })
  }
}

