import { useState, useEffect, useRef } from "react"
import { useTranslation } from 'react-i18next'
import { Search, X, Loader2 } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { api } from "@/lib/api"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import type { SearchResult, SearchSuggestion } from "@/types"
import { useNavigate } from "react-router-dom"

interface SearchBarProps {
  placeholder?: string
}

export function SearchBar({ placeholder }: SearchBarProps) {
  const { t } = useTranslation('common')
  const { handleError } = useErrorHandler()
  const defaultPlaceholder = placeholder || t('searchBar.placeholder')
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState("")
  const [results, setResults] = useState<SearchResult[]>([])
  const [suggestions, setSuggestions] = useState<SearchSuggestion[]>([])
  const [loading, setLoading] = useState(false)
  const [selectedIndex, setSelectedIndex] = useState(-1)
  const inputRef = useRef<HTMLInputElement>(null)
  const navigate = useNavigate()

  // Reset search when popover closes
  useEffect(() => {
    if (!open) {
      setQuery("")
      setResults([])
      setSuggestions([])
      setSelectedIndex(-1)
    }
  }, [open])

  // Fetch suggestions as user types
  useEffect(() => {
    if (query.length < 2) {
      setSuggestions([])
      return
    }

    const timer = setTimeout(async () => {
      try {
        const response = await api.getSearchSuggestions(query)
        setSuggestions(response.suggestions.slice(0, 5))
      } catch (error) {
        handleError(error, { operation: 'Fetch search suggestions', showToast: false })
      }
    }, 300)

    return () => clearTimeout(timer)
  }, [query])

  // Perform search when user presses Enter
  const performSearch = async (searchQuery: string) => {
    if (!searchQuery.trim()) return

    setLoading(true)
    try {
      const response = await api.globalSearch(searchQuery)
      setResults(response.results)
    } catch (error) {
      handleError(error, { operation: 'Global search', showToast: false })
    } finally {
      setLoading(false)
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault()
      setSelectedIndex((prev) =>
        prev < suggestions.length - 1 ? prev + 1 : prev
      )
    } else if (e.key === "ArrowUp") {
      e.preventDefault()
      setSelectedIndex((prev) => (prev > 0 ? prev - 1 : -1))
    } else if (e.key === "Enter") {
      if (selectedIndex >= 0 && selectedIndex < suggestions.length) {
        setQuery(suggestions[selectedIndex].text)
        performSearch(suggestions[selectedIndex].text)
      } else {
        performSearch(query)
      }
    } else if (e.key === "Escape") {
      setOpen(false)
    }
  }

  const handleResultClick = (result: SearchResult) => {
    setOpen(false)
    // Navigate based on result type
    switch (result.type) {
      case "device":
        navigate("/devices")
        break
      case "rule":
        navigate("/automation")
        break
      case "alert":
        navigate("/alerts")
        break
    }
  }

  const getTypeLabel = (type: SearchResult["type"]) => {
    const labels: Record<string, string> = {
      device: t('searchBar.typeDevice'),
      rule: t('searchBar.typeRule'),
      alert: t('searchBar.typeAlert'),
    }
    return labels[type] || type
  }

  const getTypeColor = (type: SearchResult["type"]) => {
    const colors = {
      device: "bg-blue-500",
      rule: "bg-purple-500",
      alert: "bg-red-500",
    }
    return colors[type] || "bg-gray-500"
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          className="relative w-full justify-start text-sm text-muted-foreground"
          onClick={() => setOpen(true)}
        >
          <Search className="mr-2 h-4 w-4" />
          <span className="truncate">{defaultPlaceholder}</span>
          <kbd className="pointer-events-none absolute right-1.5 top-1.5 hidden h-6 select-none items-center gap-1 rounded border bg-muted px-1.5 font-mono text-[10px] font-medium opacity-100 sm:flex">
            <span className="text-xs">⌘</span>K
          </kbd>
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[400px] p-0" align="start">
        <div className="flex items-center border-b px-3 py-2">
          <Search className="h-4 w-4 shrink-0 opacity-50" />
          <Input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={defaultPlaceholder}
            className="flex h-8 w-full rounded-none border-0 bg-transparent py-2 pl-2 pr-0 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-0 focus-visible:ring-offset-0"
            autoFocus
          />
          {query && (
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              onClick={() => setQuery("")}
            >
              <X className="h-3 w-3" />
            </Button>
          )}
        </div>

        <ScrollArea className="max-h-[400px]">
          {loading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          ) : results.length > 0 ? (
            <div className="p-2">
              <div className="px-2 py-1.5 text-xs font-medium text-muted-foreground">
                {t('searchBar.results', { count: results.length })}
              </div>
              {results.map((result, idx) => (
                <button
                  key={result.id}
                  className={`flex w-full items-center gap-3 rounded-md px-2 py-2 text-left text-sm hover:bg-accent ${
                    idx === selectedIndex ? "bg-accent" : ""
                  }`}
                  onClick={() => handleResultClick(result)}
                >
                  <span
                    className={`h-2 w-2 rounded-full ${getTypeColor(result.type)}`}
                  />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium truncate">{result.title}</span>
                      <span
                        className={`text-xs px-1.5 py-0.5 rounded text-white ${getTypeColor(
                          result.type
                        )}`}
                      >
                        {getTypeLabel(result.type)}
                      </span>
                    </div>
                    {result.description && (
                      <p className="text-xs text-muted-foreground truncate">
                        {result.description}
                      </p>
                    )}
                  </div>
                </button>
              ))}
            </div>
          ) : suggestions.length > 0 && !results.length ? (
            <div className="p-2">
              <div className="px-2 py-1.5 text-xs font-medium text-muted-foreground">
                {t('searchBar.suggestions')}
              </div>
              {suggestions.map((suggestion, idx) => (
                <button
                  key={`${suggestion.text}-${suggestion.type}`}
                  className={`flex w-full items-center justify-between gap-3 rounded-md px-2 py-2 text-left text-sm hover:bg-accent ${
                    idx === selectedIndex ? "bg-accent" : ""
                  }`}
                  onClick={() => {
                    setQuery(suggestion.text)
                    performSearch(suggestion.text)
                  }}
                >
                  <span className="truncate">{suggestion.text}</span>
                  <span className="text-xs text-muted-foreground">
                    {suggestion.count}
                  </span>
                </button>
              ))}
            </div>
          ) : query.length >= 2 ? (
            <div className="py-8 text-center text-sm text-muted-foreground">
              {t('searchBar.noResults')}
            </div>
          ) : (
            <div className="py-8 text-center text-sm text-muted-foreground">
              {t('searchBar.startTyping')}
            </div>
          )}
        </ScrollArea>
      </PopoverContent>
    </Popover>
  )
}
