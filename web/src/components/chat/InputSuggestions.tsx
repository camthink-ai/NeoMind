/**
 * InputSuggestions - Intelligent context-aware suggestions
 * Shows relevant prompts based on:
 * - Current time context (morning/evening)
 * - Actual devices in the system
 * - Recent user operations
 * - Learned patterns from agents
 * Fully internationalized with i18n
 */

import { useMemo, useEffect, useState, useCallback, useRef } from "react"
import { useTranslation, Trans } from "react-i18next"
import {
  Lightbulb,
  Cpu,
  Zap,
  AlertTriangle,
  Settings,
  TrendingUp,
  History,
  Bot,
  LucideIcon,
  Badge,
  Clock,
} from "lucide-react"
import { cn } from "@/lib/utils"

// Icon mapping for backend icon names
const ICON_MAP: Record<string, LucideIcon> = {
  Lightbulb,
  Cpu,
  Zap,
  AlertTriangle,
  Settings,
  TrendingUp,
  History,
  Bot,
}

interface BackendSuggestion {
  id: string
  label: string
  prompt: string
  icon: string
  category: string
  priority?: number
  context?: string
}

interface SuggestionsResponse {
  suggestions: BackendSuggestion[]
  context: {
    timestamp: number
    learned_patterns_count: number
    device_count: number
    time_context?: string
  }
}

interface Suggestion {
  id: string
  label: string
  prompt: string
  icon: React.ReactNode
  category: string
  priority?: number
  context?: string
}

// Fallback suggestions (minimal, used only when API completely fails)
const FALLBACK_SUGGESTIONS: BackendSuggestion[] = [
  {
    id: "help",
    label: "help", // Will be translated via getCategoryName
    prompt: "你能做什么",
    icon: "Lightbulb",
    category: "general",
    priority: 50
  }
]

interface InputSuggestionsProps {
  input: string
  onSelect: (prompt: string) => void
  visible: boolean
}

export function InputSuggestions({ input, onSelect, visible }: InputSuggestionsProps) {
  const { t } = useTranslation(["common", "chat"])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [backendSuggestions, setBackendSuggestions] = useState<BackendSuggestion[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [apiError, setApiError] = useState(false)
  const [suggestionsContext, setSuggestionsContext] = useState<SuggestionsResponse["context"] | null>(null)

  // Debounce timer ref to prevent excessive API calls
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const debouncedInputRef = useRef(input)

  // Fetch suggestions from backend API (memoized to avoid recreation)
  const fetchSuggestions = useCallback(async (searchInput: string) => {
    setIsLoading(true)
    setApiError(false)
    try {
      // Use correct API base for Tauri environment
      const apiBase = (window as any).__TAURI__ ? 'http://localhost:3000/api' : '/api'
      const response = await fetch(`${apiBase}/suggestions?input=${encodeURIComponent(searchInput)}&limit=20`)
      if (response.ok) {
        const data: SuggestionsResponse = await response.json()
        setBackendSuggestions(data.suggestions)
        setSuggestionsContext(data.context)
      } else {
        throw new Error('API request failed')
      }
    } catch (error) {
      console.error('Failed to fetch suggestions:', error)
      setApiError(true)
      setBackendSuggestions(FALLBACK_SUGGESTIONS)
    } finally {
      setIsLoading(false)
    }
  }, [])

  // Fetch immediately when component becomes visible
  useEffect(() => {
    if (visible) {
      fetchSuggestions(input)
      debouncedInputRef.current = input
    }
  }, [visible])

  // Debounced input changes - 300ms delay to reduce API calls by ~85%
  useEffect(() => {
    // Clear any pending timer
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current)
    }

    // Only debounce if visible and input actually changed
    if (visible && input !== debouncedInputRef.current) {
      debounceTimerRef.current = setTimeout(() => {
        debouncedInputRef.current = input
        fetchSuggestions(input)
      }, 300)
    }

    // Cleanup on unmount
    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current)
      }
    }
  }, [visible, input, fetchSuggestions])

  // Convert backend suggestions to frontend format with icon components
  const allSuggestions: Suggestion[] = useMemo(() => {
    return backendSuggestions.map(s => {
      const IconComponent = ICON_MAP[s.icon] || Lightbulb
      return {
        id: s.id,
        label: s.label,
        prompt: s.prompt,
        icon: <IconComponent className="h-4 w-4" />,
        category: s.category,
        priority: s.priority,
        context: s.context,
      }
    })
  }, [backendSuggestions])

  // Group suggestions by priority tier for visual distinction
  const highPrioritySuggestions = useMemo(() =>
    allSuggestions.filter(s => (s.priority ?? 0) >= 70),
    [allSuggestions]
  )

  const normalPrioritySuggestions = useMemo(() =>
    allSuggestions.filter(s => (s.priority ?? 0) < 70),
    [allSuggestions]
  )

  const filteredSuggestions = useMemo(() => {
    return [...highPrioritySuggestions, ...normalPrioritySuggestions]
  }, [highPrioritySuggestions, normalPrioritySuggestions])

  // Reset selected index when filtered suggestions change
  useEffect(() => {
    setSelectedIndex(0)
  }, [filteredSuggestions])

  // Handle keyboard navigation
  useEffect(() => {
    if (!visible) return

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault()
        setSelectedIndex(prev =>
          prev < filteredSuggestions.length - 1 ? prev + 1 : prev
        )
      } else if (e.key === "ArrowUp") {
        e.preventDefault()
        setSelectedIndex(prev => (prev > 0 ? prev - 1 : 0))
      } else if (e.key === "Enter" && filteredSuggestions.length > 0) {
        e.preventDefault()
        onSelect(filteredSuggestions[selectedIndex].prompt)
      }
    }

    window.addEventListener("keydown", handleKeyDown)
    return () => window.removeEventListener("keydown", handleKeyDown)
  }, [visible, filteredSuggestions, selectedIndex, onSelect])

  // Get category display name
  const getCategoryName = (category: string) => {
    return t(`inputSuggestions.categories.${category}`, { defaultValue: category })
  }

  // Get time greeting from context
  const getTimeGreeting = () => {
    const timeContext = suggestionsContext?.time_context
    if (!timeContext) return ""
    return t(`inputSuggestions.greeting.${timeContext}`, {
      defaultValue: t(`welcome.greeting.${timeContext}`)
    })
  }

  if (!visible || filteredSuggestions.length === 0) {
    return null
  }

  const timeGreeting = getTimeGreeting()
  const deviceCountText = suggestionsContext?.device_count !== undefined
    ? t("inputSuggestions.devices", { count: suggestionsContext.device_count })
    : ""

  return (
    <div className="absolute bottom-full left-0 right-0 mb-2 z-10">
      <div className={cn(
        "bg-[var(--popover)] border border-[var(--border)] rounded-lg",
        "shadow-lg overflow-hidden",
        "animate-in slide-in-from-bottom-2 duration-200"
      )}>
        {/* Header with time context and device count */}
        {(timeGreeting || deviceCountText) && (
          <div className="px-3 py-2 border-b border-[var(--border)] flex items-center justify-between">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              {timeGreeting && (
                <>
                  <Clock className="h-3 w-3" />
                  <span>{timeGreeting}</span>
                </>
              )}
              {timeGreeting && deviceCountText && <span className="text-border">•</span>}
              {deviceCountText && <span>{deviceCountText}</span>}
            </div>
            {isLoading && (
              <span className="text-xs text-muted-foreground">{t("inputSuggestions.loading")}</span>
            )}
          </div>
        )}

        {/* High priority suggestions section */}
        {highPrioritySuggestions.length > 0 && (
          <>
            {input.trim() && (
              <div className="px-3 py-1.5 border-b border-[var(--border)] bg-[var(--muted)]/30">
                <p className="text-xs text-muted-foreground flex items-center gap-1.5">
                  <Badge className="h-3 w-3" />
                  {t("inputSuggestions.recommended")}
                </p>
              </div>
            )}
            <div className="max-h-48 overflow-y-auto">
              {highPrioritySuggestions.map((suggestion, index) => (
                <SuggestionItem
                  key={suggestion.id}
                  suggestion={suggestion}
                  index={index}
                  selectedIndex={selectedIndex}
                  onSelect={() => onSelect(suggestion.prompt)}
                  onMouseEnter={() => setSelectedIndex(index)}
                  getCategoryName={getCategoryName}
                  t={t}
                />
              ))}
            </div>
          </>
        )}

        {/* Normal priority suggestions section */}
        {normalPrioritySuggestions.length > 0 && highPrioritySuggestions.length > 0 && (
          <div className="px-3 py-1.5 border-b border-[var(--border)] bg-[var(--muted)]/20">
            <p className="text-xs text-muted-foreground">
              {t("inputSuggestions.moreOptions")}
            </p>
          </div>
        )}

        <div className="max-h-48 overflow-y-auto">
          {normalPrioritySuggestions.map((suggestion, index) => (
            <SuggestionItem
              key={suggestion.id}
              suggestion={suggestion}
              index={index + highPrioritySuggestions.length}
              selectedIndex={selectedIndex}
              onSelect={() => onSelect(suggestion.prompt)}
              onMouseEnter={() => setSelectedIndex(index + highPrioritySuggestions.length)}
              getCategoryName={getCategoryName}
              t={t}
            />
          ))}
        </div>

        {/* Loading skeleton */}
        {isLoading && filteredSuggestions.length === 0 && (
          <div className="p-3 space-y-2">
            {[1, 2, 3].map((i) => (
              <div key={i} className="flex items-center gap-3">
                <div className="h-6 w-6 rounded-md bg-[var(--muted)] animate-pulse" />
                <div className="flex-1">
                  <div className="h-4 w-32 bg-[var(--muted)] animate-pulse rounded mb-1" />
                  <div className="h-3 w-24 bg-[var(--muted)] animate-pulse rounded" />
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Keyboard hint footer */}
        {!isLoading && filteredSuggestions.length > 0 && (
          <div className="px-3 py-1.5 border-t border-[var(--border)] bg-[var(--muted)]/30">
            <p className="text-xs text-muted-foreground">
              {t("inputSuggestions.keyboardHint")}
            </p>
          </div>
        )}
      </div>
    </div>
  )
}

// Separate component for suggestion item to reduce complexity
interface SuggestionItemProps {
  suggestion: Suggestion
  index: number
  selectedIndex: number
  onSelect: () => void
  onMouseEnter: () => void
  getCategoryName: (category: string) => string
  t: (key: string) => string
}

function SuggestionItem({ suggestion, index, selectedIndex, onSelect, onMouseEnter, getCategoryName, t }: SuggestionItemProps) {
  const isSelected = index === selectedIndex
  const isHighPriority = (suggestion.priority ?? 0) >= 70
  const categoryName = getCategoryName(suggestion.category)

  return (
    <button
      onClick={onSelect}
      onMouseEnter={onMouseEnter}
      className={cn(
        "w-full flex items-center gap-3 px-3 py-2.5 text-left",
        "transition-colors duration-150",
        isSelected
          ? "bg-[var(--accent)] text-[var(--accent-foreground)]"
          : "hover:bg-[var(--accent)]/50"
      )}
    >
      <div className={cn(
        "flex-shrink-0 p-1.5 rounded-md",
        isSelected
          ? "bg-[var(--accent)]/30"
          : isHighPriority
            ? "bg-[var(--primary)]/10"
            : "bg-[var(--muted)]"
      )}>
        {suggestion.icon}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <p className="text-sm font-medium truncate">{suggestion.label}</p>
          {isHighPriority && !isSelected && (
            <span className="flex-shrink-0 px-1.5 py-0.5 rounded-full bg-[var(--primary)]/10 text-[var(--primary)] text-[10px]">
              {t("chat.suggestions.badge")}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <p className="text-xs text-muted-foreground truncate">{suggestion.prompt}</p>
          {suggestion.context && !isSelected && (
            <>
              <span className="text-border">•</span>
              <span className="text-xs text-muted-foreground/70 truncate">{suggestion.context}</span>
            </>
          )}
        </div>
      </div>

      {/* Category badge */}
      {!isSelected && (
        <span className="flex-shrink-0 px-1.5 py-0.5 rounded text-[10px] bg-[var(--muted)] text-muted-foreground">
          {categoryName}
        </span>
      )}
    </button>
  )
}
