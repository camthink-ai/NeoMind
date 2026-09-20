/**
 * Code Editor Component
 *
 * Wraps @uiw/react-codemirror with syntax highlighting for JavaScript/TypeScript.
 * Supports light/dark themes via CSS variables (no JS theme switching) — colors
 * come from the --syntax-* tokens defined in index.css, so the editor tracks
 * the active theme automatically with zero React re-renders.
 */

import React from 'react'
import CodeMirror, { ReactCodeMirrorRef } from '@uiw/react-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { EditorView } from '@codemirror/view'
import { cn } from '@/lib/utils'
import { fontMonoStack } from '@/design-system/tokens/typography'

export interface CodeEditorProps {
  /** Current code value */
  value: string
  /** Callback when code changes */
  onChange: (value: string) => void
  /** Editor height */
  height?: string
  /** Minimum height */
  minHeight?: string
  /** Maximum height */
  maxHeight?: string
  /** Placeholder text */
  placeholder?: string
  /** Whether to show line numbers */
  lineNumbers?: boolean
  /** Read-only mode */
  readOnly?: boolean
  /** CSS class name */
  className?: string
  /** Whether to use compact mode */
  compact?: boolean
}

/**
 * CodeMirror theme driven entirely by CSS variables. The editor therefore
 * follows the active light/dark theme without any JS detection or re-render.
 * See `--syntax-*` tokens in index.css for the color source-of-truth.
 */
const editorTheme = EditorView.theme({
  // Base editor styles
  '&': {
    fontSize: '14px',
    color: 'var(--syntax-text)',
  },
  // Scroller (main content area)
  '.cm-scroller': {
    fontFamily: fontMonoStack,
    lineHeight: '1.6',
  },
  // Content padding (compact mode applied via inline override on the wrapper)
  '.cm-content': {
    padding: '14px',
  },
  // Line spacing
  '.cm-line': {
    padding: '0 0',
  },
  // Gutter (line numbers)
  '.cm-gutters': {
    backgroundColor: 'transparent',
    color: 'var(--syntax-gutter)',
    border: 'none',
    paddingRight: '8px',
    marginRight: '4px',
    fontSize: '13px',
  },
  // Current line highlight
  '.cm-activeLine': {
    backgroundColor: 'var(--syntax-active-line)',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'transparent',
    color: 'var(--syntax-text)',
    fontWeight: '500',
  },
  // Selection
  '.cm-selectionBackground': {
    backgroundColor: 'var(--syntax-selection)',
  },
  // Placeholder
  '.cm-placeholder': {
    color: 'var(--syntax-gutter)',
    fontStyle: 'italic',
  },
  // Cursor
  '.cm-cursorLayer': {
    color: 'var(--syntax-cursor)',
  },
  // Matching bracket
  '.cm-matchingBracket': {
    color: 'var(--syntax-bracket)',
    borderBottom: '2px solid var(--syntax-bracket)',
  },
  // Token classes (used by CodeMirror's default JS highlighter)
  '.tok-keyword': {
    color: 'var(--syntax-keyword)',
    fontWeight: '500',
  },
  '.tok-string': {
    color: 'var(--syntax-string)',
  },
  '.tok-number': {
    color: 'var(--syntax-number)',
  },
  '.tok-comment': {
    color: 'var(--syntax-comment)',
    fontStyle: 'italic',
  },
  '.tok-variableName': {
    color: 'var(--syntax-text)',
  },
  '.tok-def': {
    color: 'var(--syntax-def)',
  },
  '.tok-propertyName': {
    color: 'var(--syntax-property)',
  },
  '.tok-operator': {
    color: 'var(--syntax-keyword)',
  },
  '.tok-punctuation': {
    color: 'var(--syntax-text)',
  },
  '.tok-bool': {
    color: 'var(--syntax-number)',
  },
  '.tok-null': {
    color: 'var(--syntax-keyword)',
  },
  // Scrollbar styling
  '&::-webkit-scrollbar': {
    width: '8px',
    height: '8px',
  },
  '&::-webkit-scrollbar-track': {
    background: 'transparent',
  },
  '&::-webkit-scrollbar-thumb': {
    background: 'var(--syntax-scrollbar)',
    borderRadius: '4px',
  },
  '&::-webkit-scrollbar-thumb:hover': {
    background: 'var(--syntax-scrollbar-hover)',
  },
})

/**
 * Compact-mode overrides — applied as a separate theme extension so the base
 * theme stays untouched and compact is opt-in per instance.
 */
const compactTheme = EditorView.theme({
  '&': { fontSize: '13px' },
  '.cm-content': { padding: '10px 14px' },
  '.cm-gutters': { fontSize: '12px' },
})

export const CodeEditor = React.forwardRef<ReactCodeMirrorRef, CodeEditorProps>(
  ({
    value,
    onChange,
    height = 'auto',
    minHeight = '120px',
    maxHeight,
    placeholder,
    lineNumbers = true,
    readOnly = false,
    className,
    compact = false,
  }, ref) => {
    return (
      <div className={cn(
        'relative rounded-lg border overflow-hidden transition-all flex flex-col',
        'focus-within:ring-2 focus-within:ring-ring focus-within:ring-offset-2',
        'bg-background border-input',
        readOnly && 'opacity-60 cursor-not-allowed',
        className
      )}>
        <CodeMirror
          ref={ref}
          value={value}
          height={height}
          className="flex-1 min-h-0"
          minHeight={minHeight}
          maxHeight={maxHeight}
          extensions={[
            javascript({ jsx: true }),
            EditorView.lineWrapping,
            editorTheme,
            ...(compact ? [compactTheme] : []),
          ]}
          onChange={onChange}
          placeholder={placeholder}
          readOnly={readOnly}
          basicSetup={{
            lineNumbers,
            highlightActiveLineGutter: true,
            highlightSpecialChars: false,
            foldGutter: false,
            drawSelection: true,
            dropCursor: false,
            allowMultipleSelections: true,
            indentOnInput: true,
            bracketMatching: true,
            closeBrackets: true,
            autocompletion: false,
            rectangularSelection: false,
            crosshairCursor: false,
            highlightActiveLine: true,
            highlightSelectionMatches: false,
            closeBracketsKeymap: true,
            searchKeymap: false,
            foldKeymap: false,
            completionKeymap: false,
            lintKeymap: false,
          }}
        />
      </div>
    )
  }
)

CodeEditor.displayName = 'CodeEditor'

// Legacy interface for backward compatibility
export interface LegacyCodeEditorProps {
  value: string
  onChange: (value: string) => void
  language?: "json" | "yaml"
  placeholder?: string
  className?: string
  readOnly?: boolean
  error?: string
}

export function LegacyCodeEditor({
  value,
  onChange,
  language: _language = "json",
  placeholder = "",
  className,
  readOnly = false,
  error,
}: LegacyCodeEditorProps) {
  // For JSON/YAML, use the original textarea-based editor
  return (
    <div className={cn("relative", className)}>
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        readOnly={readOnly}
        placeholder={placeholder}
        className={cn(
          "w-full min-h-[200px] p-4 font-mono text-sm",
          "bg-muted border border-input rounded-md",
          "focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2",
          "resize-y overflow-auto",
          readOnly && "cursor-not-allowed opacity-50",
          error && "border-error"
        )}
        spellCheck={false}
      />
      {error && (
        <p className="text-xs text-error mt-1">{error}</p>
      )}
    </div>
  )
}
