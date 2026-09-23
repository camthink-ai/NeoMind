/**
 * Shared Tauri window-drag handlers.
 *
 * Explicitly calls startDragging() on mousedown instead of relying on
 * data-tauri-drag-region (unreliable in Tauri 2 overlay mode). Only fires
 * when clicking non-interactive areas — buttons/links/inputs are skipped so
 * controls keep working inside drag regions.
 */

import { isTauriEnv } from "@/lib/api"
import { getCurrentWindow } from "@tauri-apps/api/window"

const INTERACTIVE_SELECTOR = [
  "button", "a", "input", "select", "textarea", "label", "summary",
  "[role='button']", "[role='tab']", "[role='option']", "[role='menuitem']",
  "[role='menuitemcheckbox']", "[role='menuitemradio']", "[role='switch']",
  "[role='combobox']", "[role='slider']", "[role='scrollbar']", "[role='row']",
  "[contenteditable='true']",
  // opt-out for interactive regions the selector can't express
  "[data-no-window-drag]",
].join(", ")

export function handleWindowDragMouseDown(e: React.MouseEvent) {
  if (!isTauriEnv()) return
  const target = e.target as HTMLElement
  if (target.closest(INTERACTIVE_SELECTOR)) return
  getCurrentWindow().startDragging()
}

/** Height of the content-area drag strip (h-14). Kept in sync with the
 *  absolute strip rendered in App.tsx. */
const TOP_DRAG_STRIP_PX = 56

/**
 * Global top-edge drag for Tauri.
 *
 * The per-element drag strip in App.tsx (z-10) sits *under* portal'd dialogs
 * (Radix overlays are z-50), so with a fullscreen dialog open the top of the
 * window could no longer be dragged at all. Rather than teaching every
 * overlay about dragging, install one capture-phase pointerdown listener on
 * window: pressing a non-interactive point within the top strip starts a
 * window drag no matter which layer is on top.
 *
 * - stopPropagation keeps the event away from Radix DismissableLayer (it
 *   listens for document pointerdown to outside-close dialogs) — dragging
 *   the top edge must not close the dialog under the cursor.
 * - preventDefault suppresses the compatibility mousedown, so the per-element
 *   React drag handlers don't fire a second startDragging on the same press.
 * - Pointer events only, mouse type only: desktop window management; touch
 *   devices keep their normal tap behaviour.
 */
export function installGlobalTauriWindowDrag(): void {
  if (!isTauriEnv()) return
  window.addEventListener(
    "pointerdown",
    (e: PointerEvent) => {
      if (e.pointerType !== "mouse" || e.button !== 0) return
      if (e.clientY > TOP_DRAG_STRIP_PX) return
      const target = e.target as HTMLElement | null
      if (!target || target.closest(INTERACTIVE_SELECTOR)) return
      e.preventDefault()
      e.stopPropagation()
      getCurrentWindow().startDragging()
    },
    true,
  )
}
