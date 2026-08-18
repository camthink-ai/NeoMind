/**
 * Release-notes markdown renderer — a separate module so the whole
 * markdown/highlight vendor chunk (~325KB) stays out of the initial graph;
 * UpdateDialog lazy-loads this on first render.
 */
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { Components } from 'react-markdown'

export function ReleaseNotes({ body, components }: { body: string; components?: Components }) {
  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
      {body}
    </ReactMarkdown>
  )
}
