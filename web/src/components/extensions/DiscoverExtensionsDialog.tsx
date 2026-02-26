import { useState } from "react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Badge } from "@/components/ui/badge"
import { useToast } from "@/hooks/use-toast"
import { useStore } from "@/store"
import { syncExtensionComponents } from "@/hooks/useExtensionComponents"
import { Loader2, Package, Check, Sparkles, X } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { ExtensionDiscoveryResult } from "@/types"

interface DiscoverExtensionsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function DiscoverExtensionsDialog({
  open,
  onOpenChange,
}: DiscoverExtensionsDialogProps) {
  const { t } = useTranslation(["extensions", "common"])
  const { toast } = useToast()
  const discoverExtensions = useStore((state) => state.discoverExtensions)
  const registerExtension = useStore((state) => state.registerExtension)
  const fetchExtensions = useStore((state) => state.fetchExtensions)

  const [discovering, setDiscovering] = useState(false)
  const [discoveredExtensions, setDiscoveredExtensions] = useState<ExtensionDiscoveryResult[]>([])
  const [selectedExtensions, setSelectedExtensions] = useState<Set<string>>(new Set())
  const [registeringIds, setRegisteringIds] = useState<Set<string>>(new Set())

  // Auto-discover when dialog opens
  const handleOpenChange = (newOpen: boolean) => {
    onOpenChange(newOpen)
    if (newOpen) {
      handleDiscover()
    } else {
      // Reset state when closing
      setDiscoveredExtensions([])
      setSelectedExtensions(new Set())
      setRegisteringIds(new Set())
    }
  }

  const handleDiscover = async () => {
    setDiscovering(true)
    setDiscoveredExtensions([])
    setSelectedExtensions(new Set())
    try {
      const result = await discoverExtensions()
      setDiscoveredExtensions(result.results)
      if (result.discovered === 0) {
        toast({
          title: t("noExtensionsDiscovered"),
          description: "No extensions found in common directories",
          variant: "destructive",
        })
      }
    } catch (error) {
      toast({
        title: t("actionFailed"),
        description: error instanceof Error ? error.message : "Unknown error",
        variant: "destructive",
      })
    } finally {
      setDiscovering(false)
    }
  }

  const handleToggleExtension = (id: string) => {
    setSelectedExtensions((prev) => {
      const newSet = new Set(prev)
      if (newSet.has(id)) {
        newSet.delete(id)
      } else {
        newSet.add(id)
      }
      return newSet
    })
  }

  const handleSelectAll = () => {
    if (selectedExtensions.size === discoveredExtensions.length) {
      setSelectedExtensions(new Set())
    } else {
      setSelectedExtensions(new Set(discoveredExtensions.map((ext) => ext.id)))
    }
  }

  const handleRegisterSelected = async () => {
    if (selectedExtensions.size === 0) {
      toast({
        title: t("noExtensionSelected"),
        description: t("selectExtensionToRegister"),
        variant: "destructive",
      })
      return
    }

    const selected = discoveredExtensions.filter((ext) =>
      selectedExtensions.has(ext.id)
    )

    let successCount = 0
    const errors: { name: string; message: string }[] = []

    for (const ext of selected) {
      setRegisteringIds((prev) => new Set(prev).add(ext.id))
      try {
        await registerExtension({
          file_path: ext.file_path || "",
          auto_start: false,
        })
        successCount++
      } catch (error) {
        let errorMessage = "Unknown error"
        if (error instanceof Error) {
          errorMessage = error.message
        } else if (typeof error === "string") {
          errorMessage = error
        }

        // Provide user-friendly message for common errors
        if (errorMessage.includes("already registered") || errorMessage.includes("Already registered")) {
          errorMessage = "Already registered"
        } else if (errorMessage.includes("not found") || errorMessage.includes("NotFound")) {
          errorMessage = "File not found"
        } else if (errorMessage.includes("Failed to load extension")) {
          errorMessage = errorMessage.replace("Failed to load extension: ", "")
        }

        errors.push({ name: ext.name, message: errorMessage })
      } finally {
        setRegisteringIds((prev) => {
          const newSet = new Set(prev)
          newSet.delete(ext.id)
          return newSet
        })
      }
    }

    // Show results
    if (successCount > 0 && errors.length === 0) {
      toast({
        title: t("extensionsRegistered"),
        description: t("registeredCount", { count: successCount }),
      })
    } else if (successCount > 0 && errors.length > 0) {
      toast({
        title: t("partialSuccess"),
        description: `${successCount} succeeded, ${errors.length} failed: ${errors.map(e => e.name).join(", ")}`,
        variant: "destructive",
      })
    } else {
      toast({
        title: t("registerFailed"),
        description: errors.map(e => `${e.name}: ${e.message}`).join("; "),
        variant: "destructive",
      })
    }

    // Refresh extensions list and close dialog on success
    if (successCount > 0) {
      await fetchExtensions()
      // Sync extension components to update Component Library
      await syncExtensionComponents()
      onOpenChange(false)
    }
  }

  const handleRegisterSingle = async (ext: ExtensionDiscoveryResult) => {
    setRegisteringIds((prev) => new Set(prev).add(ext.id))
    try {
      await registerExtension({
        file_path: ext.file_path || "",
        auto_start: false,
      })

      toast({
        title: t("registerSuccess"),
        description: `${ext.name} has been registered.`,
      })
      // Remove from discovered list
      setDiscoveredExtensions((prev) => prev.filter((e) => e.id !== ext.id))
      setSelectedExtensions((prev) => {
        const newSet = new Set(prev)
        newSet.delete(ext.id)
        return newSet
      })

      // Sync extension components to update Component Library
      await syncExtensionComponents()

      // Close dialog if no more extensions
      if (discoveredExtensions.length === 1) {
        onOpenChange(false)
      }
    } catch (error) {
      // Extract meaningful error message
      let errorMessage = "Unknown error"
      if (error instanceof Error) {
        errorMessage = error.message
      } else if (typeof error === "string") {
        errorMessage = error
      }

      // Provide user-friendly message for common errors
      if (errorMessage.includes("already registered") || errorMessage.includes("Already registered")) {
        errorMessage = `${ext.name} is already registered.`
      } else if (errorMessage.includes("not found") || errorMessage.includes("NotFound")) {
        errorMessage = `Extension file not found: ${ext.file_path}`
      } else if (errorMessage.includes("Failed to load extension")) {
        errorMessage = `Failed to load ${ext.name}: ${errorMessage.replace("Failed to load extension: ", "")}`
      }

      toast({
        title: t("registerFailed"),
        description: errorMessage,
        variant: "destructive",
      })
    } finally {
      setRegisteringIds((prev) => {
        const newSet = new Set(prev)
        newSet.delete(ext.id)
        return newSet
      })
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-[600px] sm:max-h-[90vh] overflow-hidden flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 truncate">
            <Sparkles className="h-5 w-5 shrink-0" />
            {t("discover")}
          </DialogTitle>
          <DialogDescription className="truncate">
            {discovering
              ? t("discovering")
              : discoveredExtensions.length > 0
              ? t("selectExtensionsToRegister")
              : "Scanning for available extensions..."}
          </DialogDescription>
        </DialogHeader>

        <DialogContentBody className="flex-1 overflow-y-auto px-4 pt-6 pb-4 sm:px-6">
          {/* Loading State */}
          {discovering && (
            <div className="flex flex-col items-center justify-center py-8">
              <Loader2 className="h-12 w-12 animate-spin text-muted-foreground mb-4" />
              <p className="text-sm text-muted-foreground">
                {t("discovering")}...
              </p>
            </div>
          )}

          {/* Empty State */}
          {!discovering && discoveredExtensions.length === 0 && (
            <div className="flex flex-col items-center justify-center py-8 px-4">
              <Sparkles className="h-12 w-12 text-muted-foreground mb-4" />
              <h3 className="text-lg font-semibold mb-2">{t("noExtensionsDiscovered")}</h3>
              <p className="text-sm text-muted-foreground text-center max-w-md">
                No extensions found in common directories. Place extension files in:
              </p>
              <ul className="text-xs text-muted-foreground text-center mt-4 space-y-1 max-w-full">
                <li><code className="break-all">~/.neomind/extensions/</code></li>
                <li><code className="break-all">./extensions/</code></li>
                <li><code className="break-all">examples/extensions/*/target/release/</code></li>
              </ul>
              <Button
                onClick={handleDiscover}
                variant="outline"
                className="mt-6"
              >
                <Sparkles className="mr-2 h-4 w-4" />
                {t("discover")}
              </Button>
            </div>
          )}

          {/* Discovered Extensions List */}
          {!discovering && discoveredExtensions.length > 0 && (
            <div className="space-y-4">
              <div className="flex items-center justify-between gap-2">
                <h3 className="text-sm font-semibold truncate">
                  {t("discoveredExtensions")} ({discoveredExtensions.length})
                </h3>
                <div className="flex gap-2">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={handleDiscover}
                    className="h-7 text-xs"
                  >
                    <Sparkles className="h-3 w-3 mr-1" />
                    {t("refresh")}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={handleSelectAll}
                    className="h-7 text-xs"
                  >
                    {selectedExtensions.size === discoveredExtensions.length
                      ? t("deselectAll", { ns: "common" })
                      : t("selectAll", { ns: "common" })}
                  </Button>
                </div>
              </div>

              <div className="space-y-2">
                {discoveredExtensions.map((ext) => {
                  const isSelected = selectedExtensions.has(ext.id)
                  const isRegistering = registeringIds.has(ext.id)

                  return (
                    <div
                      key={ext.id}
                      className={`flex items-center gap-3 p-3 rounded-lg border transition-colors ${
                        isSelected ? "bg-accent" : "bg-background"
                      }`}
                    >
                      <Checkbox
                        checked={isSelected}
                        onCheckedChange={() => handleToggleExtension(ext.id)}
                        disabled={isRegistering}
                        className="shrink-0"
                      />
                      <div className="p-2 rounded bg-gray-100 dark:bg-gray-800 shrink-0">
                        <Package className="h-4 w-4" />
                      </div>
                      <div className="flex-1 min-w-0 overflow-hidden">
                        <div className="flex items-center gap-2">
                          <p className="font-medium text-sm truncate">{ext.name}</p>
                          <Badge variant="outline" className="text-xs shrink-0">
                            {ext.version}
                          </Badge>
                        </div>
                        <p className="text-xs text-muted-foreground truncate font-mono">
                          {ext.file_path || ext.id}
                        </p>
                      </div>
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => handleRegisterSingle(ext)}
                        disabled={isRegistering}
                        className="shrink-0"
                      >
                        {isRegistering ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : (
                          <>
                            <Check className="h-4 w-4 mr-1" />
                            {t("register", { ns: "common" })}
                          </>
                        )}
                      </Button>
                    </div>
                  )
                })}
              </div>

              {/* Batch Register Button */}
              {selectedExtensions.size > 0 && (
                <Button
                  onClick={handleRegisterSelected}
                  disabled={registeringIds.size > 0}
                  className="w-full"
                >
                  {registeringIds.size > 0 ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      {t("registering")}...
                    </>
                  ) : (
                    <>
                      <Check className="mr-2 h-4 w-4" />
                      {t("registerSelected", { count: selectedExtensions.size })}
                    </>
                  )}
                </Button>
              )}
            </div>
          )}
        </DialogContentBody>
      </DialogContent>
    </Dialog>
  )
}
