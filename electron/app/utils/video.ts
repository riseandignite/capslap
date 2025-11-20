export const getVideoPath = (filename: string) => {
  if (import.meta.env.DEV) {
    return `./assets/${filename}`
  }

  return `res://videos/${filename}`
}

