/**
 * Clipboard write that works in insecure contexts.
 *
 * `navigator.clipboard` only exists in secure contexts (HTTPS / localhost /
 * Tauri). Edge deployments are commonly reached over plain HTTP on a LAN IP,
 * where every copy button would otherwise fail with "Failed to copy".
 * Fall back to the legacy textarea + execCommand('copy') trick there.
 */
export async function copyToClipboard(text: string): Promise<void> {
  if (typeof navigator !== 'undefined' && navigator.clipboard && window.isSecureContext) {
    await navigator.clipboard.writeText(text)
    return
  }
  const ta = document.createElement('textarea')
  ta.value = text
  ta.setAttribute('readonly', '')
  ta.style.position = 'fixed'
  ta.style.opacity = '0'
  document.body.appendChild(ta)
  ta.select()
  ta.setSelectionRange(0, text.length)
  const ok = document.execCommand('copy')
  document.body.removeChild(ta)
  if (!ok) throw new Error('execCommand copy failed')
}
