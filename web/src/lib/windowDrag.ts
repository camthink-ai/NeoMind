/**
 * Shared Tauri window-drag handler.
 *
 * Explicitly calls startDragging() on mousedown instead of relying on
 * data-tauri-drag-region (unreliable in Tauri 2 overlay mode). Only fires
 * when clicking non-interactive areas — buttons/links/inputs are skipped so
 * controls keep working inside drag regions.
 */

import { isTauriEnv } from "@/lib/api"
import { getCurrentWindow } from "@tauri-apps/api/window"

export function handleWindowDragMouseDown(e: React.MouseEvent) {
  if (!isTauriEnv()) return
  const target = e.target as HTMLElement
  if (target.closest("button, a, input, select, textarea, [role='button'], [role='tab']")) return
  getCurrentWindow().startDragging()
}
