import '@testing-library/jest-dom'

// Mock Electron IPC for tests
Object.defineProperty(window, 'electron', {
  value: {
    ipcRenderer: {
      invoke: vi.fn(),
      on: vi.fn(),
      removeListener: vi.fn(),
    },
  },
  writable: true,
})

// Mock ResizeObserver and IntersectionObserver (not available in happy-dom).
// These are real classes because components call them with `new`, which a
// plain vi.fn() implementation cannot serve.
class NoopObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return []
  }
}

global.ResizeObserver = NoopObserver as unknown as typeof ResizeObserver
global.IntersectionObserver = NoopObserver as unknown as typeof IntersectionObserver

// Mock matchMedia
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

// Mock scrollTo
Element.prototype.scrollTo = vi.fn()
window.scrollTo = vi.fn()
