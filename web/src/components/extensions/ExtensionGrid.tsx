import { useState, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { ExtensionCard } from "./ExtensionCard"
import { Skeleton } from "@/components/ui/skeleton"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import {
  Package,
  Search,
  X,
  Filter,
  Grid3x3,
} from "lucide-react"
import { cn } from "@/lib/utils"
import type { Extension } from "@/types"

interface ExtensionGridProps {
  extensions: Extension[]
  loading?: boolean
  onConfigure?: (id: string) => void
  onUnregister?: (id: string) => Promise<boolean>
  onReload?: (id: string) => Promise<boolean>
}

// Status filter options with counts
interface StatusOption {
  value: string
  label: string
  icon: React.ReactNode
  className: string
}

export function ExtensionGrid({
  extensions,
  loading = false,
  onConfigure,
  onUnregister,
  onReload,
}: ExtensionGridProps) {
  const { t } = useTranslation(["extensions", "common"])

  // Filter states
  const [searchQuery, setSearchQuery] = useState("")
  const [statusFilter, setStatusFilter] = useState<string>("all")

  // Clear filters
  const clearFilters = () => {
    setSearchQuery("")
    setStatusFilter("all")
  }

  // Filter extensions and compute stats
  const { filteredExtensions, stats, statusOptions, hasActiveFilters } = useMemo(() => {
    const exts = extensions || []

    // In V2 system, extensions are always active once registered
    // Only Error state indicates a problem
    const activeCount = exts.filter((ext) => ext.state !== "Error").length
    const errorCount = exts.filter((ext) => ext.state === "Error").length

    // Build status options
    const options: StatusOption[] = [
      { value: "all", label: t("allStatuses"), icon: <Grid3x3 className="h-3 w-3" />, className: "text-muted-foreground" },
      { value: "active", label: t("categories.active", { defaultValue: "Active" }), icon: <div className="w-2 h-2 rounded-full bg-green-500" />, className: "text-green-600 dark:text-green-400" },
      { value: "error", label: t("categories.error"), icon: <div className="w-2 h-2 rounded-full bg-red-500" />, className: "text-red-600 dark:text-red-400" },
    ]

    let filtered = exts

    // Search filter
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase()
      filtered = filtered.filter(
        (ext) =>
          ext.name.toLowerCase().includes(query) ||
          ext.id.toLowerCase().includes(query) ||
          ext.description?.toLowerCase().includes(query) ||
          ext.commands?.some((cmd) =>
            cmd.id.toLowerCase().includes(query) ||
            cmd.display_name.toLowerCase().includes(query)
          )
      )
    }

    // Status filter
    if (statusFilter !== "all") {
      filtered = filtered.filter((ext) => {
        switch (statusFilter) {
          case "active":
            return ext.state !== "Error"
          case "error":
            return ext.state === "Error"
          default:
            return true
        }
      })
    }

    const computedStats = {
      total: exts.length,
      active: activeCount,
      error: errorCount,
    }

    return { filteredExtensions: filtered, stats: computedStats, statusOptions: options, hasActiveFilters: searchQuery || statusFilter !== "all" }
  }, [extensions, searchQuery, statusFilter, t])

  if (loading) {
    return (
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-5">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="border rounded-xl p-5 space-y-4">
            <div className="flex items-start gap-3">
              <Skeleton className="h-10 w-10 rounded-xl" />
              <div className="flex-1 space-y-2">
                <Skeleton className="h-5 w-3/4" />
                <Skeleton className="h-3 w-1/2" />
              </div>
            </div>
            <Skeleton className="h-16 w-full" />
          </div>
        ))}
      </div>
    )
  }

  if (!extensions || extensions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 px-4">
        <div className="p-4 rounded-full bg-muted/50 mb-4">
          <Package className="h-12 w-12 text-muted-foreground" />
        </div>
        <h3 className="text-lg font-semibold mb-2">{t("noExtensions", { defaultValue: "No Extensions" })}</h3>
        <p className="text-sm text-muted-foreground text-center max-w-md mb-6">
          {t("noExtensionsDesc", { defaultValue: "Install extensions to add new capabilities to NeoMind." })}
        </p>
      </div>
    )
  }

  // Empty state for filtered results
  if (filteredExtensions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 px-4">
        <div className="p-4 rounded-full bg-muted/50 mb-4">
          <Search className="h-12 w-12 text-muted-foreground" />
        </div>
        <h3 className="text-lg font-semibold mb-2">{t("noMatchingExtensions", { defaultValue: "No Matching Extensions" })}</h3>
        <p className="text-sm text-muted-foreground mb-6">
          {t("tryDifferentFilters", { defaultValue: "Try adjusting your search or filters" })}
        </p>
        <Button onClick={clearFilters} variant="outline">
          <X className="h-4 w-4 mr-2" />
          {t("clearFilters", { ns: "common" })}
        </Button>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* Enhanced Search and Filter Bar */}
      <div className="flex flex-col gap-4">
        {/* Search Bar */}
        <div className="flex gap-3">
          <div className="relative flex-1">
            <Search className="absolute left-3.5 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              placeholder={t("searchPlaceholder", { defaultValue: "Search extensions by name, ID, or description..." })}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-10 h-10 bg-background"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery("")}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
              >
                <X className="h-4 w-4" />
              </button>
            )}
          </div>
        </div>

        {/* Status Filter Pills */}
        <div className="flex items-center gap-2 flex-wrap">
          <div className="flex items-center gap-1.5 text-sm text-muted-foreground mr-2">
            <Filter className="h-4 w-4" />
            <span>{t("filterByStatus", { defaultValue: "Filter by:" })}</span>
          </div>
          {statusOptions.map((option) => {
            const isSelected = statusFilter === option.value
            const count = option.value === "all" ? stats.total :
                         option.value === "active" ? stats.active : stats.error

            return (
              <button
                key={option.value}
                onClick={() => setStatusFilter(option.value)}
                className={cn(
                  "inline-flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium transition-all",
                  isSelected
                    ? "bg-primary text-primary-foreground shadow-sm"
                    : "bg-muted/50 hover:bg-muted text-muted-foreground"
                )}
              >
                {option.icon}
                <span>{option.label}</span>
                <Badge variant={isSelected ? "secondary" : "outline"} className={cn(
                  "h-5 px-1.5 text-xs",
                  isSelected && "bg-primary-foreground/20 text-primary-foreground"
                )}>
                  {count}
                </Badge>
              </button>
            )
          })}
          {hasActiveFilters && (
            <Button variant="ghost" size="sm" onClick={clearFilters} className="h-8 text-xs">
              <X className="h-3 w-3 mr-1" />
              {t("clearFilters", { ns: "common" })}
            </Button>
          )}
        </div>
      </div>

      {/* Results Summary */}
      {!hasActiveFilters && (
        <div className="flex items-center justify-between text-sm">
          <div className="flex items-center gap-4">
            <span className="text-muted-foreground">
              {t("showingResults", { count: filteredExtensions.length, defaultValue: "Showing {{count}} extensions" }).replace("{{count}}", String(filteredExtensions.length))}
            </span>
            {stats.active > 0 && (
              <span className="flex items-center gap-1 text-green-600 dark:text-green-400">
                <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
                {stats.active} {t("active", { defaultValue: "active" })}
              </span>
            )}
          </div>
        </div>
      )}

      {/* Extension Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-5">
        {filteredExtensions.map((extension) => (
          <ExtensionCard
            key={extension.id}
            extension={extension}
            onConfigure={() => onConfigure?.(extension.id)}
            onUnregister={() => onUnregister?.(extension.id)}
            onReload={() => onReload?.(extension.id)}
          />
        ))}
      </div>

      {/* Filter Active Summary */}
      {hasActiveFilters && (
        <div className="flex items-center justify-between text-sm pt-2 border-t">
          <div className="flex items-center gap-2">
            <span className="text-muted-foreground">
              {t("filteredResults", { count: filteredExtensions.length, total: stats.total }).replace("{{count}}", String(filteredExtensions.length)).replace("{{total}}", String(stats.total))}
            </span>
          </div>
        </div>
      )}
    </div>
  )
}
