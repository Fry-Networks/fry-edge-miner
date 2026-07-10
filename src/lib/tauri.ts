import { invoke } from '@tauri-apps/api/core'

export function isTauri(): boolean {
  return typeof window !== 'undefined' && !!(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__
}

export async function safeInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args)
}

export async function invokeWithFallback<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  fallback: T
): Promise<T> {
  try {
    return await invoke<T>(command, args)
  } catch (err) {
    console.warn(`[tauri] ${command} failed, using fallback:`, err)
    return fallback
  }
}
