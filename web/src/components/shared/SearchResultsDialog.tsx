import { useEffect, useState } from "react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Badge } from "@/components/ui/badge"
import { Search, Loader2, FileText, Wrench, Activity, Workflow, AlertTriangle } from "lucide-react"
import { api } from "@/lib/api"
import type { SearchResult } from "@/types"
import { useNavigate } from "react-router-dom"

interface SearchResultsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  initialQuery?: string
}

export function SearchResultsDialog({ open, onOpenChange, initialQuery = "" }: SearchResultsDialogProps) {
  const [query, setQuery] = useState(initialQuery)
  const [results, setResults] = useState<SearchResult[]>([])
  const [loading, setLoading] = useState(false)
  const [hasSearched, setHasSearched] = useState(false)
  const navigate = useNavigate()

  useEffect(() => {
    if (open && initialQuery) {
      setQuery(initialQuery)
      performSearch(initialQuery)
    } else if (!open) {
      setQuery("")
      setResults([])
      setHasSearched(false)
    }
  }, [open, initialQuery])

  const performSearch = async (searchQuery: string) => {
    if (!searchQuery.trim()) return

    setLoading(true)
    setHasSearched(true)
    try {
      const response = await api.globalSearch(searchQuery)
      setResults(response.results)
    } catch (error) {
      console.error("Search failed:", error)
    } finally {
      setLoading(false)
    }
  }

  const handleResultClick = (result: SearchResult) => {
    onOpenChange(false)
    // Navigate based on result type
    switch (result.type) {
      case "device":
        navigate("/devices")
        break
      case "rule":
        navigate("/automation")
        break
      case "scenario":
        navigate("/automation")
        break
      case "workflow":
        navigate("/automation")
        break
      case "alert":
        navigate("/alerts")
        break
    }
  }

  const handleSearch = () => {
    performSearch(query)
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      performSearch(query)
    }
  }

  const getTypeIcon = (type: SearchResult["type"]) => {
    const icons = {
      device: <Wrench className="h-4 w-4" />,
      rule: <FileText className="h-4 w-4" />,
      scenario: <Activity className="h-4 w-4" />,
      workflow: <Workflow className="h-4 w-4" />,
      alert: <AlertTriangle className="h-4 w-4" />,
    }
    return icons[type] || <FileText className="h-4 w-4" />
  }

  const getTypeLabel = (type: SearchResult["type"]) => {
    const labels = {
      device: "设备",
      rule: "规则",
      scenario: "场景",
      workflow: "工作流",
      alert: "告警",
    }
    return labels[type] || type
  }

  const getTypeColor = (type: SearchResult["type"]) => {
    const colors = {
      device: "bg-blue-500/10 text-blue-500 border-blue-500/20",
      rule: "bg-purple-500/10 text-purple-500 border-purple-500/20",
      scenario: "bg-green-500/10 text-green-500 border-green-500/20",
      workflow: "bg-orange-500/10 text-orange-500 border-orange-500/20",
      alert: "bg-red-500/10 text-red-500 border-red-500/20",
    }
    return colors[type] || "bg-gray-500/10 text-gray-500 border-gray-500/20"
  }

  // Group results by type
  const groupedResults = results.reduce((acc, result) => {
    if (!acc[result.type]) {
      acc[result.type] = []
    }
    acc[result.type].push(result)
    return acc
  }, {} as Record<string, SearchResult[]>)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Search className="h-5 w-5" />
            全局搜索
          </DialogTitle>
          <DialogDescription>
            搜索设备、规则、场景、工作流和告警
          </DialogDescription>
        </DialogHeader>

        {/* Search Input */}
        <div className="flex gap-2">
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="输入搜索关键词..."
            className="flex-1"
            autoFocus
          />
          <Button onClick={handleSearch} disabled={loading || !query.trim()}>
            {loading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Search className="h-4 w-4" />
            )}
          </Button>
        </div>

        {/* Results */}
        <ScrollArea className="flex-1">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          ) : hasSearched && results.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-center">
              <Search className="h-12 w-12 text-muted-foreground mb-4" />
              <p className="text-lg font-medium">未找到匹配结果</p>
              <p className="text-sm text-muted-foreground">
                尝试使用不同的关键词进行搜索
              </p>
            </div>
          ) : !hasSearched ? (
            <div className="flex flex-col items-center justify-center py-12 text-center text-muted-foreground">
              <Search className="h-12 w-12 mb-4 opacity-50" />
              <p>输入关键词并点击搜索按钮</p>
            </div>
          ) : (
            <div className="space-y-4 pr-4">
              {Object.entries(groupedResults).map(([type, typeResults]) => (
                <div key={type}>
                  <div className="flex items-center gap-2 mb-2">
                    {getTypeIcon(type as SearchResult["type"])}
                    <span className="font-medium text-sm">
                      {getTypeLabel(type as SearchResult["type"])}
                    </span>
                    <Badge variant="secondary" className="text-xs">
                      {typeResults.length}
                    </Badge>
                  </div>
                  <div className="space-y-1 ml-6">
                    {typeResults.map((result) => (
                      <button
                        key={result.id}
                        className="flex w-full items-center gap-3 rounded-md p-2 text-left text-sm hover:bg-accent transition-colors"
                        onClick={() => handleResultClick(result)}
                      >
                        <div className="flex-1 min-w-0">
                          <div className="font-medium truncate">{result.title}</div>
                          {result.description && (
                            <div className="text-xs text-muted-foreground truncate">
                              {result.description}
                            </div>
                          )}
                        </div>
                        {result.relevance_score !== undefined && (
                          <Badge
                            variant="outline"
                            className={`text-xs ${getTypeColor(
                              result.type
                            )}`}
                          >
                            {(result.relevance_score * 100).toFixed(0)}%
                          </Badge>
                        )}
                      </button>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </ScrollArea>

        {/* Footer */}
        {hasSearched && results.length > 0 && (
          <div className="text-xs text-center text-muted-foreground pt-2 border-t">
            找到 {results.length} 条结果
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
