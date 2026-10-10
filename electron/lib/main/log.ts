/**
 * Logging for the main process.
 *
 * The default output is meant to stay readable in a terminal you are actually
 * watching: startup, failures, and nothing else. The chatty lines — every RPC
 * call, every progress tick, every base64 preview frame — are kept behind
 * `CAPSLAP_DEBUG`, because a single filmstrip renders dozens of images and each
 * one is hundreds of kilobytes of base64.
 */

export const isDebug = process.env.CAPSLAP_DEBUG === '1' || process.env.CAPSLAP_DEBUG === 'true'

/** Chatter: only when debugging. */
export function debug(...args: unknown[]): void {
  if (isDebug) console.log(...args)
}

/** Worth seeing every run: startup, shutdown, and other one-off events. */
export function info(...args: unknown[]): void {
  console.log(...args)
}

/** Always shown. */
export function warn(...args: unknown[]): void {
  console.warn(...args)
}

/** Always shown. */
export function error(...args: unknown[]): void {
  console.error(...args)
}
