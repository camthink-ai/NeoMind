import { useState, useEffect } from "react"
import { useTranslation } from 'react-i18next'
import { X, Save, Copy } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

interface FullScreenEditorProps {
  open: boolean
  onClose: () => void
  title: string
  value: string
  onChange: (value: string) => void
  onSave?: () => void
  readOnly?: boolean
}

export function FullScreenEditor({
  open,
  onClose,
  title,
  value,
  onChange,
  onSave,
  readOnly = false,
}: FullScreenEditorProps) {
  const { t } = useTranslation('common')
  const [localValue, setLocalValue] = useState(value)

  useEffect(() => {
    setLocalValue(value)
  }, [value])

  const handleSave = () => {
    onChange(localValue)
    onSave?.()
  }

  const handleCopy = () => {
    navigator.clipboard.writeText(localValue)
  }

  if (!open) return null

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <div className="w-full h-full max-w-6xl max-h-[90vh] bg-background flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b">
          <h2 className="text-lg font-semibold">{title}</h2>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="icon"
              onClick={handleCopy}
              title={t('copy')}
            >
              <Copy className="h-4 w-4" />
            </Button>
            {!readOnly && (
              <Button
                variant="ghost"
                size="icon"
                onClick={handleSave}
                title={t('save')}
              >
                <Save className="h-4 w-4" />
              </Button>
            )}
            <Button
              variant="ghost"
              size="icon"
              onClick={onClose}
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
        </div>

        {/* Editor */}
        <div className="flex-1 overflow-hidden">
          <textarea
            value={localValue}
            onChange={(e) => setLocalValue(e.target.value)}
            readOnly={readOnly}
            className={cn(
              "w-full h-full p-4 font-mono text-sm resize-none",
              "focus:outline-none",
              readOnly && "cursor-default"
            )}
            spellCheck={false}
          />
        </div>
      </div>
    </div>
  )
}
