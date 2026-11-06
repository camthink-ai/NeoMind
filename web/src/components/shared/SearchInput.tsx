/**
 * SearchInput — the canonical list-search box (DESIGN_SPEC "search right
 * anchored to the content's right edge").
 *
 * One implementation of the pattern every list page hand-rolled before:
 * search glyph left, `h-9` input with `pl-9 bg-card`, clear (×) button when
 * non-empty. Controlled component — debounce and server wiring stay at the
 * call site.
 */
import { Search, X, Loader2 } from "lucide-react"
import { useTranslation } from "react-i18next"
import { Input } from "@/components/ui/input"
import { cn } from "@/lib/utils"

export interface SearchInputProps {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  /** Width/layout classes for the wrapping box (default: full width). */
  className?: string
  /** Hide the clear button (rare — e.g. the box sits inside a popover). */
  hideClear?: boolean
  /** True while a debounced/server search is in flight — the glyph spins. */
  pending?: boolean
  autoFocus?: boolean
  "aria-label"?: string
}

export function SearchInput({
  value,
  onChange,
  placeholder,
  className,
  hideClear,
  pending,
  ...rest
}: SearchInputProps) {
  const { t } = useTranslation("common")

  return (
    <div className={cn("relative w-full", className)}>
      {pending ? (
        <Loader2 className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none animate-spin" />
      ) : (
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
      )}
      <Input
        type="search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="h-11 bg-card pl-9 pr-11 md:h-9 md:pr-8"
        {...rest}
      />
      {!hideClear && value && (
        <button
          onClick={() => onChange("")}
          aria-label={t("clear", { defaultValue: "Clear" })}
          className="absolute right-0 top-1/2 flex h-11 w-11 -translate-y-1/2 items-center justify-center text-muted-foreground transition-colors hover:text-foreground md:right-1 md:h-9 md:w-9"
        >
          <X className="h-4 w-4" />
        </button>
      )}
    </div>
  )
}
