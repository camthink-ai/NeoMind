/**
 * Token estimation with the backend's weights (CJK ≈1.8 tokens/char,
 * ASCII ≈0.25/char, ×1.1 buffer). The naive chars/3 underestimated Chinese
 * ~5x, which made the composer's context ring look like it RESET on every
 * send. Shared by both chat surfaces; a real measured count
 * (`lastTokenUsage` from the end event) should be preferred whenever one
 * exists — this is the fallback between/without replies.
 */
export function estimateTokens(text: string): number {
  let cjk = 0, ascii = 0, digits = 0, special = 0
  for (const ch of text) {
    const cp = ch.codePointAt(0) ?? 0
    if ((cp >= 0x4e00 && cp <= 0x9fff) || (cp >= 0x3400 && cp <= 0x4dbf) || (cp >= 0x3000 && cp <= 0x303f) || (cp >= 0xff00 && cp <= 0xffef)) cjk++
    else if (ch >= '0' && ch <= '9') digits++
    else if (/[a-zA-Z]/.test(ch)) ascii++
    else special++
  }
  return Math.ceil((cjk * 1.8 + ascii * 0.25 + digits * 0.3 + special * 0.5) * 1.1)
}
