import React, { useState, useEffect, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { SearchInput } from "@/components/shared"
import { useToast } from "@/components/ui/use-toast"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
  FullScreenDialogMain,
} from "@/components/automation/dialog"
import {
  Loader2,
  Download,
  Check,
  Globe,
  Package,
  Settings,
  Info,
  AlertCircle,
  ChevronRight,
  ChevronLeft,
  ExternalLink,
  X,
  FileText,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import { api } from "@/lib/api"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { useStore } from "@/store"
import { useIsMobile } from "@/hooks/useMobile"
import { categoryIcon } from "./categoryIcon"

// ============================================================================
// TYPES
// ============================================================================

interface CloudExtension {
  id: string
  name: string
  description: string
  version: string
  author: string
  license: string
  categories: string[]
  homepage?: string | null
  metadata_url?: string | null
  package_url?: string | null
}

interface MarketplaceListResponse {
  extensions: CloudExtension[]
  total: number
  market_version?: string
  error?: string
  message?: string
}

interface MarketplaceInstallResponse {
  extension_id: string
  downloaded: boolean
  installed: boolean
  path?: string | null
}

interface FullExtensionMetadata {
  id: string
  name: string
  description: string
  version: string
  author: string
  license: string
  categories: string[]
  homepage?: string | null
  repository?: string | null
  readme_url?: string | null
  package_url?: string | null
  capabilities: {
    tools: Array<{
      name: string
      display_name: string
      description: string
      parameters: Record<string, unknown>
      returns?: string | null
    }>
    metrics: Array<{
      name: string
      display_name: string
      data_type: string
      unit: string
      description?: string | null
    }>
    commands: Array<{
      name: string
      display_name: string
      description: string
      parameters: Record<string, unknown>
    }>
  }
  requirements: {
    min_neomind_version: string
    network: boolean
    api_keys: string[]
  }
  safety: {
    timeout_seconds: number
    max_memory_mb: number
  }
}

interface MarketplaceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onInstallComplete?: (extensionId: string) => void
}

// ============================================================================
// MARKETPLACE DIALOG
// ============================================================================

export function MarketplaceDialog({
  open,
  onOpenChange,
  onInstallComplete,
}: MarketplaceDialogProps) {
  const { t } = useTranslation(["extensions", "common"])
  const { toast } = useToast()
  const extensions = useStore((state) => state.extensions)
  const fetchExtensions = useStore((state) => state.fetchExtensions)
  const isMobile = useIsMobile()

  // UI state
  const [loading, setLoading] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [installingId, setInstallingId] = useState<string | null>(null)
  const [extensionsList, setExtensionsList] = useState<CloudExtension[]>([])
  const [filteredExtensions, setFilteredExtensions] = useState<CloudExtension[]>([])
  const [selectedExtension, setSelectedExtension] = useState<FullExtensionMetadata | null>(null)
  const [showDetail, setShowDetail] = useState(false)
  const [searchQuery, setSearchQuery] = useState("")
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null)

  // Load extensions when dialog opens
  useEffect(() => {
    if (open) {
      loadMarketplaceExtensions()
    }
  }, [open])

  // Filter extensions based on search and category
  useEffect(() => {
    let filtered = extensionsList

    if (searchQuery) {
      const query = searchQuery.toLowerCase()
      filtered = filtered.filter(
        (ext) =>
          ext.name.toLowerCase().includes(query) ||
          ext.description.toLowerCase().includes(query) ||
          ext.id.toLowerCase().includes(query)
      )
    }

    if (selectedCategory) {
      filtered = filtered.filter((ext) =>
        ext.categories.includes(selectedCategory)
      )
    }

    setFilteredExtensions(filtered)
  }, [extensionsList, searchQuery, selectedCategory])

  const loadMarketplaceExtensions = async () => {
    setLoading(true)
    try {
      const res = await api.get<MarketplaceListResponse>("/extensions/market/list")
      setExtensionsList(res.extensions || [])
      setFilteredExtensions(res.extensions || [])

      if (res.error && (res.extensions?.length === 0)) {
        toast({
          title: res.message || t("extensions:market.loadFailed", "Failed to load"),
          description: res.error === "network_error"
            ? t("extensions:market.networkError", "Unable to connect to GitHub. Please check your internet connection.")
            : t("extensions:market.loadFailedDesc", "Unable to load marketplace extensions"),
          variant: "destructive",
        })
      }
    } catch (e) {
      console.error("Failed to load marketplace extensions:", e)
      toast({
        title: t("extensions:market.loadFailed", "Failed to load"),
        description: t("extensions:market.loadFailedDesc", "Unable to load marketplace extensions"),
        variant: "destructive",
      })
    } finally {
      setLoading(false)
    }
  }

  const loadExtensionDetails = async (id: string) => {
    try {
      const res = await api.get<FullExtensionMetadata>(`/extensions/market/${id}`)
      setSelectedExtension(res)
      setShowDetail(true)
    } catch (e) {
      console.error("Failed to load extension details:", e)
      toast({
        title: t("extensions:market.detailsFailed", "Failed to load details"),
        variant: "destructive",
      })
    }
  }

  const handleInstall = async (id: string) => {
    setInstalling(true)
    setInstallingId(id)

    try {
      await api.post<MarketplaceInstallResponse>(
        "/extensions/market/install",
        { id },
        { skipErrorToast: true, skipRetry: true }
      )

      toast({
        title: t("extensions:market.installSuccess", "Extension installed successfully"),
        description: t("extensions:market.installSuccessDesc", "{{name}} has been installed", {
          name: extensionsList.find((e) => e.id === id)?.name || id,
        }),
      })

      await fetchExtensions()

      try {
        const { syncExtensionComponents } = await import('@/hooks/useExtensionComponents')
        await syncExtensionComponents()
      } catch (e) {
        console.warn('[Marketplace] Failed to sync extension components:', e)
      }

      onInstallComplete?.(id)

      setTimeout(() => {
        onOpenChange(false)
        setShowDetail(false)
        setSelectedExtension(null)
      }, 1000)
    } catch (e) {
      console.error("Failed to install extension:", e)
      toast({
        title: t("extensions:market.installFailed", "Installation failed"),
        description: (e instanceof Error ? e.message : null) || t("extensions:market.installFailedDesc", "Failed to install extension"),
        variant: "destructive",
      })
    } finally {
      setInstalling(false)
      setInstallingId(null)
    }
  }

  const handleClose = useCallback(() => {
    if (!installing) {
      onOpenChange(false)
      setShowDetail(false)
      setSelectedExtension(null)
    }
  }, [installing, onOpenChange])

  const isInstalled = (id: string) => {
    return extensions.some((ext) => ext.id === id)
  }

  const getAllCategories = () => {
    const categories = new Set<string>()
    extensionsList.forEach((ext) => {
      ext.categories.forEach((cat) => categories.add(cat))
    })
    return Array.from(categories).sort()
  }

  const handleBack = () => {
    setShowDetail(false)
    setSelectedExtension(null)
  }


  const showInstallFooter = showDetail && selectedExtension && !isInstalled(selectedExtension.id)

  return (
    <FullScreenDialog open={open} onOpenChange={(v) => { if (!v) handleClose(); else onOpenChange(v) }}>
      {showDetail ? (
        <header className="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-border px-4 md:px-6">
          <div className="flex min-w-0 items-center gap-2">
            <Button
              type="button"
              variant="ghost"
              size={isMobile ? "icon" : "sm"}
              onClick={handleBack}
              disabled={installing}
              className="-ml-2 shrink-0"
            >
              <ChevronLeft className="h-4 w-4" />
              {!isMobile && t("common:back")}
            </Button>
            <span className="truncate text-base font-semibold text-foreground md:text-lg">
              {selectedExtension?.name}
            </span>
          </div>
          <button
            type="button"
            onClick={handleClose}
            aria-label={t("common:close", { defaultValue: "Close" })}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring md:h-9 md:w-9"
          >
            <X className="h-5 w-5" />
          </button>
        </header>
      ) : (
        <FullScreenDialogHeader
          icon={<Globe className="h-5 w-5" />}
          title={t("extensions:market.title", "Extension Marketplace")}
          onClose={handleClose}
        />
      )}
      <FullScreenDialogContent>
        <FullScreenDialogMain className={cn("p-4 md:p-6", showDetail && "bg-muted-20")}>
          {showDetail ? (
            selectedExtension ? (
              <ExtensionDetailView
                extension={selectedExtension}
                installed={isInstalled(selectedExtension.id)}
                installing={installing && installingId === selectedExtension.id}
                onInstall={() => handleInstall(selectedExtension.id)}
                onBack={handleBack}
                isMobile={isMobile}
              />
            ) : null
          ) : (
            <div className="space-y-5">
              {/* Search and Category Filter */}
              <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                <SearchInput
                  value={searchQuery}
                  onChange={setSearchQuery}
                  placeholder={t("extensions:market.searchPlaceholder", "Search extensions...")}
                  aria-label={t("extensions:market.searchPlaceholder", "Search extensions...")}
                  className="w-full lg:max-w-sm"
                />
                <div className="flex items-center gap-1.5 overflow-x-auto pb-0.5">
                  <button
                    type="button"
                    onClick={() => setSelectedCategory(null)}
                    className={cn(
                      "h-8 shrink-0 rounded-lg border px-3 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                      selectedCategory === null
                        ? "border-primary bg-primary text-primary-foreground"
                        : "border-border bg-card text-muted-foreground hover:bg-muted-30 hover:text-foreground",
                    )}
                  >
                    {t("extensions:market.allCategories", "All")}
                  </button>
                  {getAllCategories().map((cat) => (
                    <button
                      key={cat}
                      type="button"
                      onClick={() => setSelectedCategory(cat)}
                      className={cn(
                        "h-8 shrink-0 rounded-lg border px-3 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                        selectedCategory === cat
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-border bg-card text-muted-foreground hover:bg-muted-30 hover:text-foreground",
                      )}
                    >
                      {cat}
                    </button>
                  ))}
                </div>
              </div>

              {/* Extensions List */}
              {loading ? (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,280px),1fr))] gap-4">
                  {Array.from({ length: 6 }).map((_, i) => (
                    <div
                      key={i}
                      className="rounded-xl border border-border bg-card p-4 space-y-3"
                    >
                      <div className="flex items-start gap-3">
                        <Skeleton className="h-10 w-10 rounded-lg shrink-0" />
                        <div className="flex-1 space-y-2">
                          <Skeleton className="h-4 w-3/5" />
                          <Skeleton className="h-3 w-2/5" />
                        </div>
                      </div>
                      <Skeleton className="h-9 w-full" />
                      <div className="flex items-center justify-between pt-1">
                        <Skeleton className="h-4 w-20" />
                        <Skeleton className="h-4 w-16" />
                      </div>
                    </div>
                  ))}
                </div>
              ) : filteredExtensions.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-16 text-center">
                  <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-xl border border-border bg-muted-30 text-muted-foreground">
                    <Package className="h-5 w-5" strokeWidth={1.75} />
                  </div>
                  <p className="text-sm text-muted-foreground">
                    {searchQuery || selectedCategory
                      ? t("extensions:market.noResults", "No extensions found")
                      : t("extensions:market.noExtensions", "No extensions available")}
                  </p>
                  {(searchQuery || selectedCategory) && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="mt-4"
                      onClick={() => {
                        setSearchQuery("")
                        setSelectedCategory(null)
                      }}
                    >
                      <X className="mr-1.5 h-3.5 w-3.5" />
                      {t("common:clearFilters", { defaultValue: "Clear Filters" })}
                    </Button>
                  )}
                </div>
              ) : (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,280px),1fr))] gap-4">
                  {/* Same auto-fill grid as ExtensionGrid & AgentsPage */}
                  {filteredExtensions.map((ext) => {
                    const installed = isInstalled(ext.id)
                    const CategoryIcon = categoryIcon(ext.id, ext.name)
                    return (
                      <Card
                        key={ext.id}
                        role="button"
                        tabIndex={0}
                        onClick={() => loadExtensionDetails(ext.id)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault()
                            loadExtensionDetails(ext.id)
                          }
                        }}
                        className="group flex h-full cursor-pointer flex-col overflow-hidden rounded-xl border border-border bg-card p-4 shadow-sm transition-all duration-fast ease-out hover:border-[var(--border-hover)] hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                      >
                        {/* Header: monochrome icon tile + name + status/version */}
                        <div className="mb-3 flex min-w-0 items-start gap-3">
                          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-border bg-muted-30 text-foreground">
                            <CategoryIcon className="h-4 w-4" strokeWidth={1.75} />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <h3 className="truncate text-sm font-semibold text-foreground" title={ext.name}>
                                {ext.name}
                              </h3>
                              {installed && (
                                <span className={cn(textNano, "inline-flex shrink-0 items-center gap-1 rounded-full bg-success-light px-1.5 py-0.5 font-medium text-success")}>
                                  <Check className="h-2.5 w-2.5" />
                                  {t("extensions:market.installed", "Installed")}
                                </span>
                              )}
                            </div>
                            <div className="mt-0.5 flex items-center gap-2">
                              <span className={cn(textMini, "truncate font-mono text-muted-foreground")} title={ext.id}>
                                {ext.id}
                              </span>
                              <span className={cn(textNano, "shrink-0 font-mono text-muted-foreground")}>
                                v{ext.version}
                              </span>
                            </div>
                          </div>
                        </div>

                        {/* Fixed 2-line description slot for uniform grid row height */}
                        <div className="mb-3 min-h-10">
                          <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">
                            {ext.description}
                          </p>
                        </div>

                        {/* Categories */}
                        {ext.categories.length > 0 && (
                          <div className="mb-3 flex flex-wrap items-center gap-1.5">
                            {ext.categories.slice(0, 3).map((cat) => (
                              <span
                                key={cat}
                                className={cn(
                                  textNano,
                                  "rounded-full border border-glass-border bg-muted-20 px-2 py-0.5 text-muted-foreground",
                                )}
                              >
                                {cat}
                              </span>
                            ))}
                          </div>
                        )}

                        {/* Footer: author + subtle detail affordance */}
                        <div className="mt-auto flex items-center justify-between gap-2 border-t border-glass-border pt-2.5">
                          <span className={cn(textMini, "min-w-0 truncate text-muted-foreground")}>
                            {ext.author ? t("extensions:card.byAuthor", { author: ext.author, defaultValue: `By ${ext.author}` }) : ""}
                          </span>
                          <span className="inline-flex shrink-0 items-center gap-0.5 text-xs font-medium text-muted-foreground transition-colors group-hover:text-foreground">
                            {t("extensions:market.viewDetails", "View Details")}
                            <ChevronRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
                          </span>
                        </div>
                      </Card>
                    )
                  })}
                </div>
              )}
            </div>
          )}
        </FullScreenDialogMain>
      </FullScreenDialogContent>
      {showInstallFooter && (
        <FullScreenDialogFooter>
          <Button onClick={() => handleInstall(selectedExtension!.id)} disabled={installing} size="sm" className={isMobile ? "flex-1" : ""}>
            {installing ? (
              <>
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                {t("extensions:market.installing", "Installing...")}
              </>
            ) : (
              <>
                <Download className="h-4 w-4 mr-2" />
                {t("extensions:market.install", "Install")}
              </>
            )}
          </Button>
        </FullScreenDialogFooter>
      )}
    </FullScreenDialog>
  )
}

// ============================================================================
// EXTENSION DETAIL VIEW
// ============================================================================

interface ExtensionDetailViewProps {
  extension: FullExtensionMetadata
  installed?: boolean
  installing: boolean
  onInstall: () => void
  onBack: () => void
  isMobile?: boolean
}

function ExtensionDetailView({
  extension,
  installed = false,
  installing: _installing,
  onInstall: _onInstall,
  onBack: _onBack,
  isMobile,
}: ExtensionDetailViewProps) {
  const { t } = useTranslation(["extensions", "common"])
  const [readme, setReadme] = useState<string | null>(null)

  // Fetch README asynchronously. The section renders only once content arrives,
  // so a missing README (content: null) never flashes a title/skeleton that
  // then vanishes — nothing appears until there's something to show.
  useEffect(() => {
    if (!extension.id) return
    let cancelled = false
    setReadme(null)
    api
      .get<{ content: string | null }>(`/extensions/market/${extension.id}/readme`)
      .then((res) => {
        if (!cancelled) setReadme(res?.content ?? null)
      })
      .catch((e) => {
        if (!cancelled) console.warn("Failed to load README:", e)
      })
    return () => {
      cancelled = true
    }
  }, [extension.id])

  // Strip ASCII control characters and whitespace before protocol inspection.
  const normalizeRawUrl = (raw: string | undefined | null): string =>
    Array.from(raw ?? "")
      .filter((ch) => {
        const code = ch.charCodeAt(0)
        return code >= 0x20 && code !== 0x7f
      })
      .join("")
      .trim()

  // Strictly validate external link URLs: allow ONLY https:, http:, mailto:, or # anchors.
  // Explicitly rejects javascript:, data:, vbscript:, file:, etc.
  const sanitizeSafeLinkUrl = (raw: string | undefined | null): string | undefined => {
    const cleaned = normalizeRawUrl(raw)
    if (!cleaned) return undefined
    if (cleaned.startsWith("#")) return cleaned
    try {
      const parsed = new URL(cleaned)
      if (
        parsed.protocol === "https:" ||
        parsed.protocol === "http:" ||
        parsed.protocol === "mailto:"
      ) {
        return parsed.href
      }
      return undefined
    } catch {
      return undefined
    }
  }

  // Resolve a relative path inside the README to an absolute GitHub raw URL
  // (so screenshots / doc links load instead of 404'ing against the app).
  // For links: allow ONLY https:, http:, mailto:, # anchors, or safe relative paths.
  // For images: allow ONLY https:, http:, or safe relative paths.
  // Always reject javascript:, data:, vbscript:, etc.
  const resolveReadmeUrl = (
    raw: string | undefined,
    kind: "link" | "image" = "link"
  ): string | undefined => {
    const cleaned = normalizeRawUrl(raw)
    if (!cleaned) return undefined
    if (cleaned.startsWith("#")) {
      return kind === "link" ? cleaned : undefined
    }
    // Any explicit URI scheme or protocol-relative URL must pass protocol allowlist.
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(cleaned) || cleaned.startsWith("//")) {
      try {
        const parsed = new URL(cleaned.startsWith("//") ? `https:${cleaned}` : cleaned)
        if (parsed.protocol === "https:" || parsed.protocol === "http:") {
          return parsed.href
        }
        if (kind === "link" && parsed.protocol === "mailto:") {
          return parsed.href
        }
        return undefined
      } catch {
        return undefined
      }
    }
    try {
      const base = `https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions/main/extensions/${encodeURIComponent(extension.id)}/`
      const resolved = new URL(cleaned.replace(/^\.?\//, ""), base)
      return resolved.protocol === "https:" ? resolved.href : undefined
    } catch {
      return undefined
    }
  }

  const safeHomepage = sanitizeSafeLinkUrl(extension.homepage)
  const CategoryIcon = categoryIcon(extension.id, extension.name)

  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-5 pb-4">
      {/* Header Summary Card */}
      <div className="rounded-xl border border-border bg-card p-5 space-y-3">
        <div className="flex items-start gap-3.5">
          <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl border border-border bg-muted-30 text-foreground">
            <CategoryIcon className="h-5 w-5" strokeWidth={1.75} />
          </div>
          <div className="min-w-0 flex-1 space-y-1.5">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-lg font-semibold text-foreground">{extension.name}</h2>
              <Badge variant="outline" className="font-mono text-xs font-normal">
                v{extension.version}
              </Badge>
              {installed && (
                <span className="inline-flex items-center gap-1 rounded-full bg-success-light px-2 py-0.5 text-xs font-medium text-success">
                  <Check className="h-3 w-3" />
                  {t("extensions:market.installed", "Installed")}
                </span>
              )}
              {extension.package_url && (
                <Badge variant="outline" className="bg-muted-20 text-xs font-normal text-muted-foreground">
                  <Package className="mr-1 h-3 w-3" />
                  .nep
                </Badge>
              )}
              {extension.categories.slice(0, isMobile ? 2 : undefined).map((cat) => (
                <Badge key={cat} variant="outline" className="border-glass-border bg-muted-20 text-xs font-normal text-muted-foreground">
                  {cat}
                </Badge>
              ))}
            </div>
            <p className="text-sm leading-relaxed text-muted-foreground">{extension.description}</p>
            <div className="flex flex-wrap items-center gap-4 pt-1 text-xs text-muted-foreground">
              <span>{t("extensions:card.byAuthor", { author: extension.author, defaultValue: `By ${extension.author}` })}</span>
              <span>{extension.license}</span>
              <code className="font-mono">{extension.id}</code>
              {safeHomepage && (
                <a
                  href={safeHomepage}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1 text-foreground hover:underline"
                >
                  <ExternalLink className="h-3 w-3" />
                  {t("extensions:market.homepage", "Homepage")}
                </a>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Capabilities */}
      <div className="space-y-5">
        {/* Tools */}
        {extension.capabilities.tools.length > 0 && (
          <section className="space-y-2.5">
            <h3 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <Settings className="h-3.5 w-3.5" />
              {t("extensions:market.tools", "Tools")} ({extension.capabilities.tools.length})
            </h3>
            <div className="grid gap-2">
              {extension.capabilities.tools.map((tool) => (
                <div key={tool.name} className="rounded-xl border border-border bg-card p-3.5">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium text-foreground">{tool.display_name}</span>
                    <code className="rounded-md border border-border bg-muted-30 px-1.5 py-0.5 font-mono text-xs text-muted-foreground">
                      {tool.name}
                    </code>
                  </div>
                  <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                    {tool.description}
                  </p>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* Metrics */}
        {extension.capabilities.metrics.length > 0 && (
          <section className="space-y-2.5">
            <h3 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <Info className="h-3.5 w-3.5" />
              {t("extensions:market.metrics", "Metrics")} ({extension.capabilities.metrics.length})
            </h3>
            <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
              {extension.capabilities.metrics.map((metric) => (
                <div key={metric.name} className="rounded-xl border border-border bg-card p-3">
                  <div className="truncate text-sm font-medium text-foreground">{metric.display_name}</div>
                  <div className="mt-0.5 truncate font-mono text-xs text-muted-foreground">
                    {metric.name}
                  </div>
                  <div className="mt-1.5 text-xs text-muted-foreground">
                    {metric.data_type}
                    {metric.unit && ` · ${metric.unit}`}
                  </div>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* Commands */}
        {extension.capabilities.commands.length > 0 && (
          <section className="space-y-2.5">
            <h3 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <Package className="h-3.5 w-3.5" />
              {t("extensions:market.commands", "Commands")} ({extension.capabilities.commands.length})
            </h3>
            <div className="flex flex-wrap gap-2">
              {extension.capabilities.commands.map((cmd) => (
                <Badge key={cmd.name} variant="outline" className="bg-card px-3 py-1 text-xs font-normal text-foreground">
                  {cmd.display_name}
                </Badge>
              ))}
            </div>
          </section>
        )}

        {/* Requirements */}
        {(extension.requirements.network || extension.requirements.api_keys.length > 0) && (
          <section className="rounded-xl border border-border bg-card p-4 space-y-2">
            <h3 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <AlertCircle className="h-3.5 w-3.5" />
              {t("extensions:market.requirements", "Requirements")}
            </h3>
            <ul className="space-y-1 text-xs leading-relaxed text-muted-foreground">
              {extension.requirements.network && (
                <li>• {t("extensions:market.requiresNetwork", "Requires network access")}</li>
              )}
              {extension.requirements.api_keys.map((key) => (
                <li key={key}>• {key}</li>
              ))}
            </ul>
          </section>
        )}

        {/* README / Documentation — render only once content arrives, so a
            missing README never flashes a title that then vanishes. */}
        {readme && (
          <section className="rounded-xl border border-border bg-card p-5 space-y-3">
            <h3 className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <FileText className="h-3.5 w-3.5" />
              {t("extensions:market.readme", "Documentation")}
            </h3>
            <div className="prose prose-sm dark:prose-invert max-w-none">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={{
                  img: ({ node: _node, ...props }) => {
                    const safeSrc = resolveReadmeUrl(props.src, "image")
                    if (!safeSrc) return null
                    return <img {...props} src={safeSrc} alt={props.alt || 'image'} />
                  },
                  a: ({ node: _node, ...props }) => {
                    const safeHref = resolveReadmeUrl(props.href, "link")
                    if (!safeHref) {
                      return <span>{props.children}</span>
                    }
                    const isAnchor = safeHref.startsWith("#")
                    return (
                      <a
                        {...props}
                        href={safeHref}
                        target={isAnchor ? undefined : "_blank"}
                        rel={isAnchor ? undefined : "noopener noreferrer"}
                      />
                    )
                  },
                }}
              >
                {readme}
              </ReactMarkdown>
            </div>
          </section>
        )}
      </div>
    </div>
  )
}
