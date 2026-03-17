import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { PageLayout } from "@/components/layout/PageLayout"
import { ExtensionGrid, ExtensionDetailsDialog, MarketplaceDialog } from "@/components/extensions"
import { ExtensionUploadDialog } from "@/components/extensions"
import { useToast } from "@/hooks/use-toast"
import { RefreshCw, Upload, Globe } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { dynamicRegistry } from "@/components/dashboard/registry/DynamicRegistry"
import type { Extension } from "@/types"

export function ExtensionsPage() {
  const { t } = useTranslation(["extensions", "common"])
  const { toast } = useToast()

  // Use the main store to access extension state and actions
  const extensions = useStore((state) => state.extensions)
  const extensionsLoading = useStore((state) => state.extensionsLoading)
  const fetchExtensions = useStore((state) => state.fetchExtensions)
  const unregisterExtension = useStore((state) => state.unregisterExtension)
  const reloadExtension = useStore((state) => state.reloadExtension)

  const [selectedExtension, setSelectedExtension] = useState<Extension | null>(null)
  const [detailsDialogOpen, setDetailsDialogOpen] = useState(false)
  const [uploadDialogOpen, setUploadDialogOpen] = useState(false)
  const [marketplaceDialogOpen, setMarketplaceDialogOpen] = useState(false)

  // Confirmation dialogs state
  const [reloadConfirmOpen, setReloadConfirmOpen] = useState(false)
  const [uninstallConfirmOpen, setUninstallConfirmOpen] = useState(false)
  const [pendingActionExtension, setPendingActionExtension] = useState<Extension | null>(null)

  // Fetch extensions on mount
  useEffect(() => {
    fetchExtensions()
  }, [fetchExtensions])

  // Refresh handler
  const handleRefresh = async () => {
    await fetchExtensions()
    toast({
      title: t("common:refreshed"),
      variant: "default",
    })
  }

  // Extension action handlers
  const handleUninstall = async (id: string): Promise<boolean> => {
    const ext = extensions.find(e => e.id === id)
    if (!ext) return false

    setPendingActionExtension(ext)
    setUninstallConfirmOpen(true)
    return false // Will be handled by confirmation
  }

  const confirmUninstall = async () => {
    if (!pendingActionExtension) return
    const id = pendingActionExtension.id

    const result = await unregisterExtension(id)
    if (result) {
      // Clear extension's components from dynamic registry
      dynamicRegistry.unregisterExtension(id)

      toast({
        title: t("extensions:extensionUninstalled"),
      })
    } else {
      toast({
        title: t("extensions:actionFailed"),
        variant: "destructive",
      })
    }
    setUninstallConfirmOpen(false)
    setPendingActionExtension(null)
  }

  const handleConfigure = (id: string) => {
    const ext = extensions.find(e => e.id === id)
    if (ext) {
      setSelectedExtension(ext)
      setDetailsDialogOpen(true)
    }
  }

  const handleReload = async (id: string): Promise<boolean> => {
    const ext = extensions.find(e => e.id === id)
    if (!ext) return false

    setPendingActionExtension(ext)
    setReloadConfirmOpen(true)
    return false // Will be handled by confirmation
  }

  const confirmReload = async () => {
    if (!pendingActionExtension) return
    const id = pendingActionExtension.id

    const result = await reloadExtension(id)
    if (result) {
      toast({
        title: t("extensions:extensionReloaded", { defaultValue: "Extension reloaded successfully" }),
      })
    } else {
      toast({
        title: t("extensions:actionFailed"),
        variant: "destructive",
      })
    }
    setReloadConfirmOpen(false)
    setPendingActionExtension(null)
  }

  const handleUploadComplete = (extensionId: string) => {
    fetchExtensions()
    toast({
      title: t("extensions:extensionUploaded"),
    })
    // Dialog will be closed by ExtensionUploadDialog after showing success message
  }

  return (
    <>
      <PageLayout
        title={t("extensions:title", { defaultValue: "Extensions" })}
        subtitle={t("extensions:description", { defaultValue: "Manage dynamic extensions and plugins" })}
        borderedHeader={false}
      >
        {/* Header Actions */}
        <div className="flex justify-between items-center mb-6">
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setMarketplaceDialogOpen(true)}
            >
              <Globe className="h-4 w-4 mr-2" />
              {t("extensions:marketplace", { defaultValue: "Marketplace" })}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setUploadDialogOpen(true)}
            >
              <Upload className="h-4 w-4 mr-2" />
              {t("extensions:uploadExtension", { defaultValue: "Upload" })}
            </Button>
          </div>
          <Button
            variant="ghost"
            size="icon"
            onClick={handleRefresh}
            disabled={extensionsLoading}
          >
            <RefreshCw className={`h-4 w-4 ${extensionsLoading ? "animate-spin" : ""}`} />
          </Button>
        </div>

        {/* Extensions Grid */}
        <ExtensionGrid
          extensions={extensions}
          loading={extensionsLoading}
          onUninstall={handleUninstall}
          onConfigure={handleConfigure}
          onReload={handleReload}
        />
      </PageLayout>

      {/* Extension Details Dialog */}
      <ExtensionDetailsDialog
        extension={selectedExtension}
        open={detailsDialogOpen}
        onOpenChange={setDetailsDialogOpen}
      />

      {/* Upload Dialog */}
      <ExtensionUploadDialog
        open={uploadDialogOpen}
        onOpenChange={setUploadDialogOpen}
        onUploadComplete={handleUploadComplete}
      />

      {/* Marketplace Dialog */}
      <MarketplaceDialog
        open={marketplaceDialogOpen}
        onOpenChange={setMarketplaceDialogOpen}
        onInstallComplete={() => {
          fetchExtensions()
          toast({
            title: t("extensions:extensionInstalled", { defaultValue: "Extension installed successfully" }),
          })
        }}
      />

      {/* Reload Confirmation Dialog */}
      <AlertDialog open={reloadConfirmOpen} onOpenChange={setReloadConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("extensions:confirmReload", { defaultValue: "Reload Extension" })}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("extensions:confirmReloadDescription", {
                defaultValue: `Are you sure you want to reload "${pendingActionExtension?.name}"? This will reload the extension from its source file.`,
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setPendingActionExtension(null)}>
              {t("common:cancel", { defaultValue: "Cancel" })}
            </AlertDialogCancel>
            <AlertDialogAction onClick={confirmReload}>
              <RefreshCw className="h-4 w-4 mr-2" />
              {t("extensions:reload", { defaultValue: "Reload" })}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Unregister Confirmation Dialog */}
      <AlertDialog open={uninstallConfirmOpen} onOpenChange={setUninstallConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("extensions:confirmUninstall", { defaultValue: "Completely Uninstall Extension" })}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("extensions:confirmUninstallDescription", {
                defaultValue: `Are you sure you want to completely uninstall "${pendingActionExtension?.name}"? This will remove ALL extension files including source code, and this action CANNOT be undone.`,
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setPendingActionExtension(null)}>
              {t("common:cancel", { defaultValue: "Cancel" })}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={confirmUninstall}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {t("extensions:uninstall", { defaultValue: "Uninstall" })}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
