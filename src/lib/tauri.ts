import { invoke } from '@tauri-apps/api/core'

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
