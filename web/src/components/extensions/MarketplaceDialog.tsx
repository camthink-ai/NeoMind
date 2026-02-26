import React, { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { useToast } from "@/components/ui/use-toast"
import {
  Loader2,
  Download,
  Search,
  Check,
  Globe,
  Package,
  Settings,
  Info,
  AlertCircle,
  ChevronRight,
  Github,
  Star,
  ExternalLink,
  X,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { api } from "@/lib/api"
import { useStore } from "@/store"

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
  package_url?: string | null  // .nep package URL if available
}

interface MarketplaceListResponse {
  extensions: CloudExtension[]
  total: number
  market_version?: string
  error?: string
  message?: string
}

interface MarketplaceInstallResponse {
  success: boolean
  extension_id: string
  downloaded: boolean
  installed: boolean
  path?: string | null
  error?: string | null
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
  package_url?: string | null  // .nep package URL if available
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
  const [marketVersion, setMarketVersion] = useState<string>("")

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
      setMarketVersion(res.market_version || "")
      setFilteredExtensions(res.extensions || [])

      // Show warning if there was an error but still got some results
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
      const response = await api.post<MarketplaceInstallResponse>(
        "/extensions/market/install",
        { id }
      )

      if (response.success) {
        toast({
          title: t("extensions:market.installSuccess", "Extension installed successfully"),
          description: t("extensions:market.installSuccessDesc", "{{name}} has been installed", {
            name: extensionsList.find((e) => e.id === id)?.name || id,
          }),
        })

        // Refresh extensions list
        await fetchExtensions()

        // Call completion callback
        onInstallComplete?.(id)

        // Close dialog after short delay
        setTimeout(() => {
          onOpenChange(false)
          setShowDetail(false)
          setSelectedExtension(null)
        }, 1000)
      } else {
        toast({
          title: t("extensions:market.installFailed", "Installation failed"),
          description: response.error || t("extensions:market.installFailedDesc", "Failed to install extension"),
          variant: "destructive",
        })
      }
    } catch (e: any) {
      console.error("Failed to install extension:", e)
      toast({
        title: t("extensions:market.installFailed", "Installation failed"),
        description: e?.message || t("extensions:market.installFailedDesc", "Failed to install extension"),
        variant: "destructive",
      })
    } finally {
      setInstalling(false)
      setInstallingId(null)
    }
  }

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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl sm:max-h-[90vh] flex flex-col overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Globe className="h-5 w-5" />
            <span className="truncate">{t("extensions:market.title", "Extension Marketplace")}</span>
            {marketVersion && (
              <Badge variant="outline" className="text-xs shrink-0">
                v{marketVersion}
              </Badge>
            )}
          </DialogTitle>
        </DialogHeader>

        {!showDetail ? (
          <DialogContentBody className="flex-1 overflow-y-auto px-4 pt-6 pb-4 sm:px-6">
            {/* Search and Filter */}
            <div className="flex flex-col gap-3 pb-4 sm:flex-row sm:gap-3">
              <div className="relative flex-1">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                <Input
                  placeholder={t("extensions:market.searchPlaceholder", "Search extensions...")}
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-9"
                />
              </div>
              <div className="flex gap-2 flex-wrap">
                <Button
                  variant={selectedCategory === null ? "default" : "outline"}
                  size="sm"
                  onClick={() => setSelectedCategory(null)}
                >
                  {t("extensions:market.allCategories", "All")}
                </Button>
                {getAllCategories().slice(0, 5).map((cat) => (
                  <Button
                    key={cat}
                    variant={selectedCategory === cat ? "default" : "outline"}
                    size="sm"
                    onClick={() => setSelectedCategory(cat)}
                  >
                    {cat}
                  </Button>
                ))}
              </div>
            </div>

            {/* Extensions List */}
            <div className="flex-1 overflow-y-auto">
              {loading ? (
                <div className="flex items-center justify-center h-64">
                  <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
                </div>
              ) : filteredExtensions.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-64 text-center">
                  <Package className="h-12 w-12 text-muted-foreground mb-4" />
                  <p className="text-muted-foreground">
                    {searchQuery || selectedCategory
                      ? t("extensions:market.noResults", "No extensions found")
                      : t("extensions:market.noExtensions", "No extensions available")}
                  </p>
                </div>
              ) : (
                <div className="grid gap-3">
                  {filteredExtensions.map((ext) => {
                    const installed = isInstalled(ext.id)
                    return (
                      <Card
                        key={ext.id}
                        className={cn(
                          "p-4 hover:bg-accent/50 transition-colors cursor-pointer overflow-hidden",
                          installed && "border-primary/50"
                        )}
                        onClick={() => !installed && loadExtensionDetails(ext.id)}
                      >
                        <div className="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3">
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2 mb-1">
                              <h3 className="font-semibold truncate">{ext.name}</h3>
                              {installed && (
                                <Badge variant="secondary" className="text-xs">
                                  <Check className="h-3 w-3 mr-1" />
                                  {t("extensions:market.installed", "Installed")}
                                </Badge>
                              )}
                              <Badge variant="outline" className="text-xs">
                                {ext.version}
                              </Badge>
                            </div>
                            <p className="text-sm text-muted-foreground line-clamp-2 mb-2 h-10 leading-5">
                              {ext.description}
                            </p>
                            <div className="flex items-center gap-2 flex-wrap">
                              {ext.categories.slice(0, 3).map((cat) => (
                                <Badge key={cat} variant="secondary" className="text-xs">
                                  {cat}
                                </Badge>
                              ))}
                              {ext.author && (
                                <span className="text-xs text-muted-foreground">
                                  by {ext.author}
                                </span>
                              )}
                            </div>
                          </div>
                          <div className="flex items-center gap-2 sm:shrink-0 w-full sm:w-auto">
                            {installed ? (
                              <Button
                                variant="outline"
                                size="sm"
                                className="w-full sm:w-auto"
                                disabled
                                onClick={(e) => e.stopPropagation()}
                              >
                                <Check className="h-4 w-4 mr-1" />
                                {t("extensions:market.installed", "Installed")}
                              </Button>
                            ) : (
                              <Button
                                variant="default"
                                size="sm"
                                className="w-full sm:w-auto"
                                onClick={(e) => {
                                  e.stopPropagation()
                                  loadExtensionDetails(ext.id)
                                }}
                              >
                                {t("extensions:market.viewDetails", "View Details")}
                                <ChevronRight className="h-4 w-4 ml-1" />
                              </Button>
                            )}
                          </div>
                        </div>
                      </Card>
                    )
                  })}
                </div>
              )}
            </div>
          </DialogContentBody>
        ) : (
          <DialogContentBody className="flex-1 overflow-y-auto px-4 pt-6 pb-4 sm:px-6">
            {/* Detail View */}
            {selectedExtension && (
              <ExtensionDetailView
                extension={selectedExtension}
                installing={installing && installingId === selectedExtension.id}
                onInstall={() => handleInstall(selectedExtension.id)}
                onBack={() => {
                  setShowDetail(false)
                  setSelectedExtension(null)
                }}
              />
            )}
          </DialogContentBody>
        )}
      </DialogContent>
    </Dialog>
  )
}

// ============================================================================
// EXTENSION DETAIL VIEW
// ============================================================================

interface ExtensionDetailViewProps {
  extension: FullExtensionMetadata
  installing: boolean
  onInstall: () => void
  onBack: () => void
}

function ExtensionDetailView({
  extension,
  installing,
  onInstall,
  onBack,
}: ExtensionDetailViewProps) {
  const { t } = useTranslation(["extensions", "common"])

  return (
    <div className="flex flex-col h-full overflow-y-auto">
      {/* Header */}
      <div className="border-b pb-4">
        <div className="flex items-center gap-2 mb-2">
          <h2 className="text-xl font-semibold">{extension.name}</h2>
          <Badge variant="outline">{extension.version}</Badge>
          {extension.package_url && (
            <Badge variant="default" className="text-xs">
              <Package className="h-3 w-3 mr-1" />
              .nep Package
            </Badge>
          )}
          {extension.categories.map((cat) => (
            <Badge key={cat} variant="secondary">
              {cat}
            </Badge>
          ))}
        </div>
        <p className="text-muted-foreground">{extension.description}</p>
        <div className="flex items-center gap-4 mt-2 text-sm text-muted-foreground">
          <span>by {extension.author}</span>
          <span>{extension.license}</span>
          {extension.homepage && (
            <a
              href={extension.homepage}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1 hover:text-primary"
            >
              <ExternalLink className="h-3 w-3" />
              {t("extensions:market.homepage", "Homepage")}
            </a>
          )}
        </div>
      </div>

      {/* Capabilities */}
      <div className="flex-1 overflow-y-auto py-4 space-y-6">
        {/* Tools */}
        {extension.capabilities.tools.length > 0 && (
          <div>
            <h3 className="font-semibold mb-3 flex items-center gap-2">
              <Settings className="h-4 w-4" />
              {t("extensions:market.tools", "Tools")} ({extension.capabilities.tools.length})
            </h3>
            <div className="grid gap-2">
              {extension.capabilities.tools.map((tool) => (
                <Card key={tool.name} className="p-3">
                  <div className="flex items-start justify-between">
                    <div>
                      <div className="font-medium">{tool.display_name}</div>
                      <div className="text-sm text-muted-foreground">
                        <code className="text-xs bg-muted px-1 rounded">{tool.name}</code>
                      </div>
                      <p className="text-sm text-muted-foreground mt-1">
                        {tool.description}
                      </p>
                    </div>
                  </div>
                </Card>
              ))}
            </div>
          </div>
        )}

        {/* Metrics */}
        {extension.capabilities.metrics.length > 0 && (
          <div>
            <h3 className="font-semibold mb-3 flex items-center gap-2">
              <Info className="h-4 w-4" />
              {t("extensions:market.metrics", "Metrics")} ({extension.capabilities.metrics.length})
            </h3>
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
              {extension.capabilities.metrics.map((metric) => (
                <Card key={metric.name} className="p-3">
                  <div className="text-sm font-medium">{metric.display_name}</div>
                  <div className="text-xs text-muted-foreground">
                    <code>{metric.name}</code>
                  </div>
                  <div className="text-xs text-muted-foreground mt-1">
                    {metric.data_type}
                    {metric.unit && ` · ${metric.unit}`}
                  </div>
                </Card>
              ))}
            </div>
          </div>
        )}

        {/* Commands */}
        {extension.capabilities.commands.length > 0 && (
          <div>
            <h3 className="font-semibold mb-3 flex items-center gap-2">
              <Package className="h-4 w-4" />
              {t("extensions:market.commands", "Commands")} ({extension.capabilities.commands.length})
            </h3>
            <div className="flex flex-wrap gap-2">
              {extension.capabilities.commands.map((cmd) => (
                <Badge key={cmd.name} variant="secondary" className="text-sm py-1 px-3">
                  {cmd.display_name}
                </Badge>
              ))}
            </div>
          </div>
        )}

        {/* Requirements */}
        {(extension.requirements.network || extension.requirements.api_keys.length > 0) && (
          <div className="bg-muted/50 rounded-lg p-4">
            <h3 className="font-semibold mb-2 flex items-center gap-2">
              <AlertCircle className="h-4 w-4" />
              {t("extensions:market.requirements", "Requirements")}
            </h3>
            <ul className="text-sm space-y-1">
              {extension.requirements.network && (
                <li>• {t("extensions:market.requiresNetwork", "Requires network access")}</li>
              )}
              {extension.requirements.api_keys.map((key) => (
                <li key={key}>• {key}</li>
              ))}
            </ul>
          </div>
        )}
      </div>

      {/* Footer */}
      <DialogFooter className="border-t pt-4 justify-between">
        <Button variant="outline" onClick={onBack} disabled={installing}>
          {t("common:back", "Back")}
        </Button>
        <Button onClick={onInstall} disabled={installing}>
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
      </DialogFooter>
    </div>
  )
}
