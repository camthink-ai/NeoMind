import * as React from "react"
import { cn } from "@/lib/utils"

export interface InputProps
  extends React.InputHTMLAttributes<HTMLInputElement> {}

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, value: valueProp, onChange: onChangeProp, onCompositionStart: compStartProp, onCompositionEnd: compEndProp, ...props }, ref) => {
    // IME-safe: buffer value locally during composition so the controlled
    // value doesn't interrupt CJK / pinyin input.  This is especially
    // needed in Tauri (WebKit) where React's built-in composition
    // suppression doesn't always work.
    //
    // For password fields, skip all composition buffering — let the
    // browser handle type="password" natively.  IME intermediate
    // characters bypass ● masking and cause garbled display.
    const isPassword = type === 'password'
    const composingRef = React.useRef(false)
    const [buffer, setBuffer] = React.useState(isPassword ? undefined : valueProp)

    // Sync from parent when value changes externally (and not composing)
    React.useEffect(() => {
      if (!composingRef.current && !isPassword) {
        setBuffer(valueProp)
      }
    }, [valueProp, isPassword])

    if (isPassword) {
      return (
        <input
          type="password"
          className={cn(
            "flex h-10 w-full rounded-md border border-input bg-card px-3 py-2 text-sm file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:border-primary disabled:cursor-not-allowed disabled:opacity-50 aria-[invalid=true]:border-error aria-[invalid=true]:focus-visible:border-error",
            className
          )}
          style={{ imeMode: 'disabled', ...props.style }}
          ref={ref}
          value={valueProp}
          onChange={(e) => onChangeProp?.(e)}
          onCompositionStart={(e) => {
            // Prevent IME composition on password fields.
            // In Tauri (WebKit), active IME (e.g. Chinese Pinyin) produces
            // intermediate characters that bypass ● masking → garbled display.
            e.preventDefault()
            compStartProp?.(e)
          }}
          {...props}
        />
      )
    }

    return (
      <input
        type={type}
        className={cn(
          "flex h-10 w-full rounded-md border border-input bg-card px-3 py-2 text-sm file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 aria-[invalid=true]:border-error aria-[invalid=true]:focus-visible:ring-error-light",
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
          const v = (e.target as HTMLInputElement).value
          setBuffer(v)
          onChangeProp?.({ target: { value: v } } as React.ChangeEvent<HTMLInputElement>)
          compEndProp?.(e)
        }}
        {...props}
      />
    )
  }
)
Input.displayName = "Input"

export { Input }
