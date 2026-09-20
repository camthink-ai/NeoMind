// Chat tool-call helpers — split from ToolCallVisualization.tsx
// (react-refresh: component files export only components).

/** Check if thinking duplicates the content (Phase 2 LLM may echo response as "thinking") */
export function isThinkingDuplicate(thinking: string | undefined, content: string | undefined): boolean {
  if (!thinking || !content) return false
  const tPreview = thinking.slice(0, 200)
  const cPreview = content.slice(0, 200)
  if (!tPreview || !cPreview) return false
  return cPreview.includes(tPreview) || tPreview.includes(cPreview)
}
