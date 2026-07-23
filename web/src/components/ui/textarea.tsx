import * as React from "react"
import { cn } from "@/lib/utils"

export interface TextareaProps
  extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {}

const Textarea = React.forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ className, value: valueProp, onChange: onChangeProp, onCompositionStart: compStartProp, onCompositionEnd: compEndProp, ...props }, ref) => {
    // IME-safe: buffer value locally during composition so the controlled
    // value doesn't interrupt CJK / pinyin input.
    const composingRef = React.useRef(false)
    const [buffer, setBuffer] = React.useState(valueProp)

    // Sync from parent when value changes externally (and not composing)
    React.useEffect(() => {
      if (!composingRef.current) {
        setBuffer(valueProp)
      }
    }, [valueProp])

    return (
      <textarea
        className={cn(
          "flex min-h-[80px] w-full rounded-md border border-input bg-card px-3 py-2 text-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 aria-[invalid=true]:border-error aria-[invalid=true]:focus-visible:ring-error-light",
          className
        )}
        ref={ref}
        value={buffer}
        onChange={(e) => {
          setBuffer(e.target.value)
          if (!composingRef.current) {
            onChangeProp?.(e)
          }
        }}
        onCompositionStart={(e) => {
          composingRef.current = true
          compStartProp?.(e)
        }}
        onCompositionEnd={(e) => {
          composingRef.current = false
          const v = (e.target as HTMLTextAreaElement).value
          setBuffer(v)
          onChangeProp?.({ target: { value: v } } as React.ChangeEvent<HTMLTextAreaElement>)
          compEndProp?.(e)
        }}
        {...props}
      />
    )
  }
)
Textarea.displayName = "Textarea"

export { Textarea }
