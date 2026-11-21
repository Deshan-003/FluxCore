/* FluxCore toast store (UI_DESIGN_SYSTEM.md §5.10).
 * No external dependency; every completed async action reports here in
 * addition to any inline result text. */

export type ToastKind = 'success' | 'error' | 'info'

export interface ToastItem {
  id: number
  kind: ToastKind
  text: string
}

let toastSeq = 0
let toastItems: ToastItem[] = []
const toastListeners = new Set<() => void>()

function emitToasts() { toastListeners.forEach((l) => l()) }

export function toast(text: string, kind: ToastKind = 'info') {
  const item: ToastItem = { id: ++toastSeq, kind, text }
  toastItems = [...toastItems, item]
  emitToasts()
  window.setTimeout(() => {
    toastItems = toastItems.filter((t) => t.id !== item.id)
    emitToasts()
  }, 6000)
}

export function dismissToast(id: number) {
  toastItems = toastItems.filter((t) => t.id !== id)
  emitToasts()
}

export function subscribeToasts(onStoreChange: () => void) {
  toastListeners.add(onStoreChange)
  return () => { toastListeners.delete(onStoreChange) }
}

export function getToasts(): ToastItem[] {
  return toastItems
}
