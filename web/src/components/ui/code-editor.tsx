import { useEffect, useRef } from "react"
import { cn } from "@/lib/utils"

interface CodeEditorProps {
  value: string
  onChange: (value: string) => void
  language?: "json" | "yaml"
  placeholder?: string
  className?: string
  readOnly?: boolean
  error?: string
}

export function CodeEditor({
  value,
  onChange,
  language = "json",
  placeholder = "",
  className,
  readOnly = false,
  error,
}: CodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  // Handle tab key in textarea
  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Tab") {
      e.preventDefault()
      const start = e.currentTarget.selectionStart
      const end = e.currentTarget.selectionEnd
      const newValue = value.substring(0, start) + "  " + value.substring(end)
      onChange(newValue)
      setTimeout(() => {
        e.currentTarget.selectionStart = e.currentTarget.selectionEnd = start + 2
      }, 0)
    }
  }

  // Format on blur
  const handleBlur = () => {
    if (language === "json" && value.trim()) {
      try {
        const parsed = JSON.parse(value)
        const formatted = JSON.stringify(parsed, null, 2)
        if (formatted !== value) {
          onChange(formatted)
        }
      } catch {
        // Invalid JSON, don't auto-format
      }
    }
  }

  // Validate JSON/YAML
  const isValid = value.trim()
    ? (() => {
        if (language === "json") {
          try {
            JSON.parse(value)
            return true
          } catch {
            return false
          }
        }
        // YAML syntax check (basic)
        const lines = value.split("\n")
        let indent = 0
        for (const line of lines) {
          if (line.trim().startsWith("#")) continue // comments
          if (line.trim() === "") continue
          const lineIndent = line.search(/\S/)
          if (lineIndent > indent + 2) return false // indent too deep
          if (!line.startsWith(" ")) indent = lineIndent
        }
        return true
      })()
    : true

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current
    if (textarea) {
      textarea.style.height = "auto"
      textarea.style.height = textarea.scrollHeight + "px"
    }
  }, [value])

  return (
    <div className={cn("relative", className)}>
      <textarea
        ref={textareaRef}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={handleKeyDown}
        onBlur={handleBlur}
        readOnly={readOnly}
        placeholder={placeholder}
        className={cn(
          "w-full min-h-[200px] p-4 font-mono text-sm",
          "bg-muted border border-input rounded-md",
          "focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2",
          "resize-y overflow-auto",
          readOnly && "cursor-not-allowed opacity-50",
          !isValid && !readOnly && "border-destructive",
          error && "border-destructive"
        )}
        spellCheck={false}
      />
      {language === "json" && value.trim() && !isValid && !readOnly && (
        <p className="text-xs text-destructive mt-1">JSON 格式错误</p>
      )}
      {language === "yaml" && value.trim() && !isValid && !readOnly && (
        <p className="text-xs text-destructive mt-1">YAML 格式错误（缩进不正确）</p>
      )}
      {error && (
        <p className="text-xs text-destructive mt-1">{error}</p>
      )}
    </div>
  )
}
