/**
 * Memory Panel
 *
 * Displays Markdown-based memory files (USER.md + KNOWLEDGE.md + custom files).
 * Uses table layout similar to device list for consistency.
 * Includes simplified configuration UI.
 */

import { useState, useEffect, useCallback, forwardRef, useImperativeHandle } from "react"
import { useTranslation } from "react-i18next"
import ReactMarkdown from "react-markdown"
import CodeMirror from "@uiw/react-codemirror"
import {
  Eye,
  Pencil,
  Download,
  Loader2,
  User,
  BookOpen,
  ListChecks,
  Clock,
  Save,
  X,
  Settings,
  MoreVertical,
  FileText,
  Trash2,
  Plus,
} from "lucide-react"
import { Card } from "@/components/ui/card"
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { Switch } from "@/components/ui/switch"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { ResponsiveTable, EmptyState, LoadingState } from "@/components/shared"
import { Button, IconButton } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
  FullScreenDialogMain,
} from "@/components/automation/dialog"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/components/ui/use-confirm"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import { fontMonoStack, textMini } from "@/design-system/tokens/typography"
import { formatTimestamp } from "@/lib/utils/format"
import { useIsMobile } from "@/hooks/useMobile"
import type { MemorySystemConfig, LlmBackendInstance } from "@/types"

// Default config for initialization
const defaultConfig: MemorySystemConfig = {
  enabled: true,
  storage_path: "data/memory",
  user_char_limit: 2000,
  knowledge_char_limit: 3000,
  procedures_char_limit: 3000,
  agent_char_limit: 1000,
  temp_file_ttl_days: 7,
  system_context_interval_secs: 600,
  summary_interval_secs: 7200,
  summary_backend_id: null,
}

// Custom file type from stats API
interface CustomFileStat {
  name: string
  chars: number
}

// Memory files configuration
const fileConfig = [
  {
    id: "user",
    labelKey: "systemMemory.files.user",
    defaultLabel: "User Profile",
    icon: User,
    description: "User preferences, habits and personal settings",
    color: "bg-info-light text-info border-info",
    charLimitKey: "user_char_limit" as const,
  },
  {
    id: "knowledge",
    labelKey: "systemMemory.files.knowledge",
    defaultLabel: "System Knowledge",
    icon: BookOpen,
    description: "System resources, domain knowledge, and agent experiences",
    color: "bg-success-light text-success border-success-light",
    charLimitKey: "knowledge_char_limit" as const,
  },
  {
    id: "procedures",
    labelKey: "systemMemory.files.procedures",
    defaultLabel: "Procedures",
    icon: ListChecks,
    description: "SOPs, playbooks, and how-tos learned across sessions",
    color: "bg-accent-purple-light text-accent-purple border-accent-purple",
    charLimitKey: "procedures_char_limit" as const,
  },
]

// Custom file display config
const customFileConfig = {
  icon: FileText,
  color: "bg-warning-light text-warning border-warning-light",
}

// File stats from API
interface FileStats {
  chars: number
  modified_at: number
}

// Memory stats response from API
interface MemoryStatsResponse {
  files: Record<string, FileStats>
  custom_files?: CustomFileStat[]
}

// Table row data type
interface MemoryFileRow {
  id: string
  name: string
  description: string
  icon: React.ElementType
  color: string
  chars: number
  charLimit: number
  modified_at: number
  isCustom: boolean
  onDelete?: () => void
}

interface MemoryPanelProps {
  refreshKey?: number
}

export interface MemoryPanelRef {
  openConfig: () => void
  openCreateFile: () => void
}

export const MemoryPanel = forwardRef<MemoryPanelRef, MemoryPanelProps>(function MemoryPanel({ refreshKey }, ref) {
  const { t } = useTranslation("agents")
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
  const isMobile = useIsMobile()

  // State
  const [stats, setStats] = useState<Record<string, FileStats>>({})
  const [customFiles, setCustomFiles] = useState<CustomFileStat[]>([])
  const [loading, setLoading] = useState(true)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [selectedFileIsCustom, setSelectedFileIsCustom] = useState(false)
  const [content, setContent] = useState("")
  const [contentLoading, setContentLoading] = useState(false)
  const [editing, setEditing] = useState(false)
  const [editContent, setEditContent] = useState("")
  const [saving, setSaving] = useState(false)
  const [exporting, setExporting] = useState<string | null>(null)

  // Create dialog state
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [newFileName, setNewFileName] = useState("")
  const [newFileContent, setNewFileContent] = useState("")
  const [creating, setCreating] = useState(false)

  // Configuration state
  const [configOpen, setConfigOpen] = useState(false)
  const [config, setConfig] = useState<MemorySystemConfig>(defaultConfig)
  const [configLoading, setConfigLoading] = useState(false)
  const [configSaving, setConfigSaving] = useState(false)
  const [llmBackends, setLlmBackends] = useState<LlmBackendInstance[]>([])

  // Load configuration
  const loadConfig = useCallback(async () => {
    setConfigLoading(true)
    try {
      const response = await api.getMemoryConfig()
      setConfig({ ...defaultConfig, ...response })
    } catch (error) {
      handleError(error, { operation: "Load memory config", showToast: false })
    } finally {
      setConfigLoading(false)
    }
  }, [handleError])

  // Load LLM backends when config dialog opens
  useEffect(() => {
    if (configOpen) {
      api.listLlmBackends()
        .then((res) => {
          setLlmBackends(res.backends || [])
        })
        .catch((err) => {
          handleError(err, { operation: "Load LLM backends", showToast: false })
        })
    }
  }, [configOpen, handleError])

  useEffect(() => {
    loadConfig()
  }, [loadConfig])

  // Save configuration
  const handleSaveConfig = async () => {
    setConfigSaving(true)
    try {
      await api.updateMemoryConfig(config)
      setConfigOpen(false)
    } catch (error) {
      handleError(error, { operation: "Save memory config" })
    } finally {
      setConfigSaving(false)
    }
  }

  // Load stats (includes custom files from backend)
  const loadStats = useCallback(async () => {
    setLoading(true)
    try {
      const response: MemoryStatsResponse = await api.getMemoryStats()
      setStats(response.files || {})
      setCustomFiles(response.custom_files || [])
    } catch (error) {
      handleError(error, { operation: "Load stats", showToast: false })
    } finally {
      setLoading(false)
    }
  }, [handleError])

  useEffect(() => {
    loadStats()
  }, [loadStats, refreshKey])

  // Load content for a file (built-in or custom)
  const loadContent = async (fileId: string, isCustom: boolean) => {
    setContentLoading(true)
    try {
      if (isCustom) {
        const response = await api.getCustomMemoryFile(fileId)
        setContent(response.content || "")
        setEditContent(response.content || "")
      } else {
        const response = await api.getMemoryFile(fileId)
        setContent(response.content || "")
        setEditContent(response.content || "")
      }
    } catch (error) {
      handleError(error, { operation: "Load memory content" })
      setContent("")
      setEditContent("")
    } finally {
      setContentLoading(false)
    }
  }

  // Handle view/edit action
  const handleViewEdit = (fileId: string, isCustom: boolean) => {
    setSelectedFile(fileId)
    setSelectedFileIsCustom(isCustom)
    setEditing(false)
    setDialogOpen(true)
    loadContent(fileId, isCustom)
  }

  // Handle save (built-in or custom)
  const handleSave = async () => {
    if (!selectedFile) return
    setSaving(true)
    try {
      if (selectedFileIsCustom) {
        await api.updateCustomMemoryFile(selectedFile, editContent)
      } else {
        await api.updateMemoryFile(selectedFile, editContent)
      }
      setContent(editContent)
      setEditing(false)
      loadStats()
    } catch (error) {
      handleError(error, { operation: "Save memory" })
    } finally {
      setSaving(false)
    }
  }

  // Handle delete (custom files only)
  const handleDelete = async (name: string) => {
    const confirmed = await confirm({
      title: t("systemMemory.custom.delete", "Delete"),
      description: t("systemMemory.custom.deleteConfirm", `Delete file "${name}"? This cannot be undone.`, { name }),
      confirmText: t("common:delete", "Delete"),
      variant: "destructive",
    })
    if (!confirmed) return

    try {
      await api.deleteCustomMemoryFile(name)
      loadStats()
      toast({
        title: t("systemMemory.custom.deleted", "File deleted"),
        description: name,
      })
    } catch (error) {
      handleError(error, { operation: "Delete file" })
    }
  }

  // Handle create custom file
  const handleCreateCustom = async () => {
    if (!newFileName.trim()) return
    setCreating(true)
    try {
      await api.updateCustomMemoryFile(newFileName.trim(), newFileContent)
      setCreateDialogOpen(false)
      setNewFileName("")
      setNewFileContent("")
      loadStats()
      toast({
        title: t("systemMemory.custom.created", "File created"),
        description: newFileName,
      })
    } catch (error) {
      handleError(error, { operation: "Create file" })
    } finally {
      setCreating(false)
    }
  }

  // Handle export
  const handleExport = async (fileId: string) => {
    setExporting(fileId)
    try {
      const markdown = await api.exportAllMemory()
      const blob = new Blob([markdown], { type: "text/markdown" })
      const url = URL.createObjectURL(blob)
      const a = document.createElement("a")
      a.href = url
      a.download = `memory_${fileId}_${new Date().toISOString().split("T")[0]}.md`
      a.click()
      URL.revokeObjectURL(url)
    } catch (error) {
      handleError(error, { operation: "Export memory" })
    } finally {
      setExporting(null)
    }
  }

  // Handle dialog close
  const handleDialogClose = (open: boolean) => {
    setDialogOpen(open)
    if (!open) {
      setEditing(false)
    }
  }

  // Expose methods via ref
  useImperativeHandle(ref, () => ({
    openConfig: () => {
      setConfigOpen(true)
      loadConfig()
    },
    openCreateFile: () => {
      setNewFileName("")
      setNewFileContent("")
      setCreateDialogOpen(true)
    },
  }), [loadConfig])

  // Get char limit for a file from config
  const getCharLimit = (fileId: string, isCustom: boolean): number => {
    if (isCustom) return config.agent_char_limit || 1000
    const fc = fileConfig.find(f => f.id === fileId)
    if (!fc) return 0
    if (fc.charLimitKey === "user_char_limit") return config.user_char_limit || 0
    if (fc.charLimitKey === "knowledge_char_limit") return config.knowledge_char_limit || 0
    if (fc.charLimitKey === "procedures_char_limit") return config.procedures_char_limit || 0
    return 0
  }

  // Get file config by id
  const getSelectedFileConfig = () => {
    if (selectedFileIsCustom) return null
    return fileConfig.find((f) => f.id === selectedFile)
  }

  // Check for dark mode
  const isDark =
    typeof document !== "undefined" &&
    (document.documentElement.getAttribute("data-theme") === "dark" ||
      document.documentElement.classList.contains("dark"))

  // Format chars usage display
  const formatCharsUsage = (chars: number, limit: number) => {
    if (limit <= 0) return `${chars}`
    return `${chars} / ${limit}`
  }

  // Prepare table data: built-in files + custom files
  const tableData: MemoryFileRow[] = [
    ...fileConfig.map((file) => ({
      id: file.id,
      name: t(file.labelKey, file.defaultLabel),
      description: file.description,
      icon: file.icon,
      color: file.color,
      chars: stats[file.id]?.chars ?? 0,
      charLimit: getCharLimit(file.id, false),
      modified_at: stats[file.id]?.modified_at ?? 0,
      isCustom: false,
    })),
    ...customFiles.map((file) => ({
      id: `custom:${file.name}`,
      name: file.name,
      description: t("systemMemory.custom.description", "Custom knowledge file"),
      icon: customFileConfig.icon,
      color: customFileConfig.color,
      chars: file.chars,
      charLimit: getCharLimit(file.name, true),
      modified_at: 0,
      isCustom: true,
    })),
  ]

  // Dialog title
  const dialogTitle = selectedFileIsCustom
    ? (selectedFile || "")
    : (getSelectedFileConfig() ? t(getSelectedFileConfig()!.labelKey, getSelectedFileConfig()!.defaultLabel) : "")

  const dialogIcon = selectedFileIsCustom
    ? <FileText className="h-5 w-5" />
    : (() => {
        const fc = getSelectedFileConfig()
        if (!fc) return <FileText className="h-5 w-5" />
        const Icon = fc.icon
        return <Icon className="h-5 w-5" />
      })()

  const dialogIconBg = selectedFileIsCustom
    ? customFileConfig.color
    : (getSelectedFileConfig()?.color || "bg-muted")

  return (
    <div className="space-y-4">
      {/* File table */}
      {isMobile ? (
        <div className="space-y-2">
          {tableData.map((row) => {
            const Icon = row.icon
            return (
              <Card
                key={row.id}
                className="overflow-hidden border-border shadow-sm cursor-pointer active:scale-[0.99] transition-all"
                onClick={() => handleViewEdit(row.isCustom ? row.name : row.id, row.isCustom)}
              >
                <div className="px-3 py-2.5">
                  <div className="flex items-center gap-2.5">
                    <div className={cn("w-8 h-8 rounded-lg flex items-center justify-center border shrink-0", row.color)}>
                      <Icon className="h-4 w-4" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="font-medium text-sm truncate">{row.name}</div>
                    </div>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                        <IconButton aria-label={t("systemMemory.actions", "Actions")}>
                          <MoreVertical className="h-4 w-4" />
                        </IconButton>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleViewEdit(row.isCustom ? row.name : row.id, row.isCustom) }}>
                          <Eye className="h-4 w-4 mr-2" />
                          {t("systemMemory.viewEdit", "View/Edit")}
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleExport(row.id) }}>
                          <Download className="h-4 w-4 mr-2" />
                          {t("systemMemory.export", "Export")}
                        </DropdownMenuItem>
                        {row.isCustom && (
                          <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleDelete(row.name) }} className="text-error focus:text-error">
                            <Trash2 className="h-4 w-4 mr-2" />
                            {t("systemMemory.custom.delete", "Delete")}
                          </DropdownMenuItem>
                        )}
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                  <div className="flex items-center gap-1.5 mt-1.5 ml-[42px]">
                    <Badge variant="secondary" className={cn(textMini, "font-mono h-5 px-1.5")}>
                      {formatCharsUsage(row.chars, row.charLimit)} {t("systemMemory.headers.chars", "chars")}
                    </Badge>
                    {!row.isCustom && (
                      <span className={cn(textMini, "text-muted-foreground ml-auto")}>
                        {row.modified_at > 0 ? formatTimestamp(row.modified_at, false) : "-"}
                      </span>
                    )}
                  </div>
                </div>
              </Card>
            )
          })}
        </div>
      ) : (
      <ResponsiveTable
        columns={[
          {
            key: "name",
            label: t("systemMemory.headers.file", "File"),
          },
          {
            key: "chars",
            label: (
              <div className="flex items-center gap-1">
                {t("systemMemory.headers.chars", "Chars")}
              </div>
            ),
            align: "right",
            width: "w-36",
          },
          {
            key: "modified_at",
            label: (
              <div className="flex items-center gap-1">
                <Clock className="h-4 w-4" />
                {t("systemMemory.headers.modified", "Modified")}
              </div>
            ),
            align: "right",
            width: "w-32",
          },
        ]}
        data={tableData as unknown as Record<string, unknown>[]}
        rowKey={(row) => (row as unknown as MemoryFileRow).id}
        onRowClick={(row) => {
          const r = row as unknown as MemoryFileRow
          handleViewEdit(r.isCustom ? r.name : r.id, r.isCustom)
        }}
        loading={loading}
        renderCell={(columnKey, rowData) => {
          const row = rowData as unknown as MemoryFileRow
          const Icon = row.icon

          switch (columnKey) {
            case "name":
              return (
                <div className="flex items-center gap-3">
                  <div
                    className={cn(
                      "w-9 h-9 rounded-lg flex items-center justify-center border",
                      row.color
                    )}
                  >
                    <Icon className="h-4 w-4" />
                  </div>
                  <div>
                    <div className="font-medium text-sm">{row.name}</div>
                    <div className="text-xs text-muted-foreground">
                      {row.description}
                    </div>
                  </div>
                </div>
              )

            case "chars":
              return (
                <span className="text-xs text-muted-foreground font-mono whitespace-nowrap">
                  {formatCharsUsage(row.chars, row.charLimit)}
                </span>
              )

            case "modified_at":
              return (
                <span className="text-xs text-muted-foreground">
                  {row.modified_at > 0
                    ? formatTimestamp(row.modified_at, false)
                    : "-"}
                </span>
              )

            default:
              return null
          }
        }}
        actions={[
          {
            label: t("systemMemory.viewEdit", "View/Edit"),
            icon: <Eye className="h-4 w-4" />,
            onClick: (rowData) => {
              const row = rowData as unknown as MemoryFileRow
              handleViewEdit(row.isCustom ? row.name : row.id, row.isCustom)
            },
          },
          {
            label: t("systemMemory.export", "Export"),
            icon: exporting ? <Loader2 className="h-4 w-4 animate-spin" /> : <Download className="h-4 w-4" />,
            onClick: (rowData) => {
              const row = rowData as unknown as MemoryFileRow
              handleExport(row.id)
            },
          },
          {
            label: t("systemMemory.custom.delete", "Delete"),
            icon: <Trash2 className="h-4 w-4" />,
            variant: "destructive" as const,
            onClick: (rowData) => {
              const row = rowData as unknown as MemoryFileRow
              if (row.isCustom) handleDelete(row.name)
            },
            show: (rowData) => {
              const row = rowData as unknown as MemoryFileRow
              return row.isCustom
            },
          },
        ]}
        emptyState={
          <EmptyState
            icon={<FileText className="h-12 w-12" />}
            title={t('systemMemory.empty.title', 'No memory files')}
            description={t('systemMemory.empty.description', 'Memory files will appear here once configured')}
          />
        }
      />
      )}

      {/* Unified View/Edit Dialog */}
      <FullScreenDialog open={dialogOpen} onOpenChange={handleDialogClose}>
        <FullScreenDialogHeader
          icon={dialogIcon}
          iconBg={dialogIconBg}
          title={dialogTitle}
          onClose={() => handleDialogClose(false)}
        />

        <FullScreenDialogContent className="flex-col">
          {contentLoading ? (
            <LoadingState size="lg" className="flex-1" />
          ) : editing ? (
            <div className="w-full h-full overflow-hidden">
              <CodeMirror
                value={editContent}
                height="100%"
                onChange={(value) => setEditContent(value)}
                theme={isDark ? "dark" : "light"}
                style={{
                  fontSize: "14px",
                  fontFamily: fontMonoStack,
                  height: "100%",
                  width: "100%",
                }}
              />
            </div>
          ) : (
            <div className="prose prose-sm dark:prose-invert max-w-none p-6 overflow-auto h-full w-full">
              {content ? (
                <ReactMarkdown>{content}</ReactMarkdown>
              ) : (
                <p className="text-muted-foreground italic">
                  {t("systemMemory.empty", "No content yet")}
                </p>
              )}
            </div>
          )}
        </FullScreenDialogContent>

        <FullScreenDialogFooter>
          <div className="flex items-center justify-between w-full">
            <div className="text-xs text-muted-foreground">
              {content.split("\n").filter(l => l.trim()).length} {t("systemMemory.lines", "lines")}
              {selectedFile && (
                <span className="ml-2">
                  ({content.length} / {getCharLimit(selectedFile, selectedFileIsCustom)} {t("systemMemory.headers.chars", "chars")})
                </span>
              )}
            </div>
            <div className="flex items-center gap-2">
              {editing ? (
                <>
                  <Button
                    variant="outline"
                    onClick={() => {
                      setEditContent(content)
                      setEditing(false)
                    }}
                    disabled={saving}
                  >
                    <X className="h-4 w-4 mr-1" />
                    {t("systemMemory.cancel", "Cancel")}
                  </Button>
                  <Button onClick={handleSave} disabled={saving}>
                    {saving ? (
                      <Loader2 className="h-4 w-4 mr-1 animate-spin" />
                    ) : (
                      <Save className="h-4 w-4 mr-1" />
                    )}
                    {saving ? t("systemMemory.saving", "Saving...") : t("systemMemory.save", "Save")}
                  </Button>
                </>
              ) : (
                <Button onClick={() => setEditing(true)}>
                  <Pencil className="h-4 w-4 mr-1" />
                  {t("systemMemory.edit", "Edit")}
                </Button>
              )}
            </div>
          </div>
        </FullScreenDialogFooter>
      </FullScreenDialog>

      {/* Create Custom File Dialog */}
      <FullScreenDialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
        <FullScreenDialogHeader
          icon={<Plus className="h-5 w-5" />}
          iconBg="bg-success-light text-success"
          title={t("systemMemory.custom.createTitle", "Create Custom File")}
          onClose={() => setCreateDialogOpen(false)}
        />

        <FullScreenDialogContent className="flex-col">
          {/* File name — compact bar at top, full width */}
          <div className="px-6 pt-4 pb-3 border-b shrink-0 space-y-1.5">
            <Label>{t("systemMemory.custom.fileName", "File Name")}</Label>
            <Input
              value={newFileName}
              onChange={(e) => {
                const val = e.target.value.replace(/[^a-z0-9_-]/g, "")
                if (val.length <= 32) setNewFileName(val)
              }}
              placeholder="e.g. device-map"
              className="font-mono"
            />
            <p className="text-xs text-muted-foreground">
              {t("systemMemory.custom.fileNameHint", "Lowercase letters, digits, hyphens, underscores. 1-32 chars.")}
            </p>
          </div>
          {/* Content editor — fills the rest (mirrors the View/Edit dialog) */}
          <div className="w-full flex-1 min-h-0 overflow-hidden">
            <CodeMirror
              value={newFileContent}
              height="100%"
              onChange={(value) => setNewFileContent(value)}
              theme={isDark ? "dark" : "light"}
              style={{
                fontSize: "14px",
                fontFamily: fontMonoStack,
                height: "100%",
                width: "100%",
              }}
            />
          </div>
        </FullScreenDialogContent>

        <FullScreenDialogFooter>
          <Button variant="outline" onClick={() => setCreateDialogOpen(false)}>
            {t("systemMemory.cancel", "Cancel")}
          </Button>
          <Button
            onClick={handleCreateCustom}
            disabled={creating || !newFileName.trim()}
          >
            {creating ? (
              <Loader2 className="h-4 w-4 mr-1 animate-spin" />
            ) : (
              <Plus className="h-4 w-4 mr-1" />
            )}
            {creating ? t("systemMemory.saving", "Saving...") : t("systemMemory.custom.create", "Create")}
          </Button>
        </FullScreenDialogFooter>
      </FullScreenDialog>

      {/* Configuration Dialog */}
      <FullScreenDialog open={configOpen} onOpenChange={setConfigOpen}>
        <FullScreenDialogHeader
          icon={<Settings className="h-5 w-5" />}
          iconBg="bg-accent-purple-light"
          iconColor="text-accent-purple"
          title={t("systemMemory.config.title", "Memory Configuration")}
          subtitle={t("systemMemory.config.description", "Configure memory storage and scheduling")}
          onClose={() => setConfigOpen(false)}
        />

        <FullScreenDialogContent>
          {configLoading ? (
            <LoadingState size="lg" className="flex-1" />
          ) : (
            <FullScreenDialogMain className="p-6">
              <div className="space-y-8 max-w-4xl mx-auto">
                {/* General Settings */}
                <div className="space-y-4">
                  <h3 className="text-sm font-semibold flex items-center gap-2">
                    <Settings className="h-4 w-4 text-accent-purple" />
                    {t("systemMemory.config.general", "General Settings")}
                  </h3>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <Label>{t("systemMemory.config.enabled", "Enabled")}</Label>
                      <div className="flex items-center h-10">
                        <Switch
                          checked={config.enabled}
                          onCheckedChange={(checked) =>
                            setConfig({ ...config, enabled: checked })
                          }
                        />
                      </div>
                    </div>
                  </div>
                </div>

                {/* Char Limits */}
                <div className="space-y-4">
                  <h3 className="text-sm font-semibold flex items-center gap-2">
                    <BookOpen className="h-4 w-4 text-info" />
                    {t("systemMemory.config.charLimits", "Character Limits")}
                  </h3>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <Label>{t("systemMemory.config.userCharLimit", "User File Limit")}</Label>
                      <Input
                        type="number"
                        min={500}
                        max={10000}
                        step={100}
                        value={config.user_char_limit}
                        onChange={(e) =>
                          setConfig({
                            ...config,
                            user_char_limit: parseInt(e.target.value) || 2000,
                          })
                        }
                      />
                      <p className="text-xs text-muted-foreground">
                        {t("systemMemory.config.userCharLimitHint", "Max characters for user memory file")}
                      </p>
                    </div>
                    <div className="space-y-2">
                      <Label>{t("systemMemory.config.knowledgeCharLimit", "Knowledge File Limit")}</Label>
                      <Input
                        type="number"
                        min={500}
                        max={20000}
                        step={100}
                        value={config.knowledge_char_limit}
                        onChange={(e) =>
                          setConfig({
                            ...config,
                            knowledge_char_limit: parseInt(e.target.value) || 3000,
                          })
                        }
                      />
                      <p className="text-xs text-muted-foreground">
                        {t("systemMemory.config.knowledgeCharLimitHint", "Max characters for knowledge memory file")}
                      </p>
                    </div>
                    <div className="space-y-2">
                      <Label>{t("systemMemory.config.proceduresCharLimit", "Procedures File Limit")}</Label>
                      <Input
                        type="number"
                        min={500}
                        max={20000}
                        step={100}
                        value={config.procedures_char_limit}
                        onChange={(e) =>
                          setConfig({
                            ...config,
                            procedures_char_limit: parseInt(e.target.value) || 3000,
                          })
                        }
                      />
                      <p className="text-xs text-muted-foreground">
                        {t("systemMemory.config.proceduresCharLimitHint", "Max characters for procedures memory file (SOPs, playbooks, how-tos)")}
                      </p>
                    </div>
                    <div className="space-y-2">
                      <Label>{t("systemMemory.config.tempFileTtl", "Temp File TTL (Days)")}</Label>
                      <Input
                        type="number"
                        min={1}
                        max={30}
                        value={config.temp_file_ttl_days}
                        onChange={(e) =>
                          setConfig({
                            ...config,
                            temp_file_ttl_days: parseInt(e.target.value) || 7,
                          })
                        }
                      />
                      <p className="text-xs text-muted-foreground">
                        {t("systemMemory.config.tempFileTtlHint", "Days before temp files are cleaned up")}
                      </p>
                    </div>
                  </div>
                </div>

                {/* Schedule Settings */}
                <div className="space-y-4">
                  <h3 className="text-sm font-semibold flex items-center gap-2">
                    <Clock className="h-4 w-4 text-accent-orange" />
                    {t("systemMemory.config.schedule", "Schedule")}
                  </h3>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <Label>{t("systemMemory.config.refreshInterval", "Refresh Interval")}</Label>
                      <Input
                        type="number"
                        min={60}
                        max={86400}
                        step={60}
                        value={config.system_context_interval_secs}
                        onChange={(e) =>
                          setConfig({
                            ...config,
                            system_context_interval_secs: parseInt(e.target.value) || 300,
                          })
                        }
                      />
                      <p className="text-xs text-muted-foreground">
                        {t("systemMemory.config.refreshIntervalHint", "Seconds between resource inventory refresh ({{minutes}} min)", {
                          minutes: Math.round((config.system_context_interval_secs || 600) / 60),
                        })}
                      </p>
                    </div>
                    <div className="space-y-2">
                      <Label>{t("systemMemory.config.summaryInterval", "Summary Interval")}</Label>
                      <Input
                        type="number"
                        min={600}
                        max={86400}
                        step={60}
                        value={config.summary_interval_secs}
                        onChange={(e) =>
                          setConfig({
                            ...config,
                            summary_interval_secs: parseInt(e.target.value) || 3600,
                          })
                        }
                      />
                      <p className="text-xs text-muted-foreground">
                        {t("systemMemory.config.summaryIntervalHint", "Seconds between LLM chat/agent summaries ({{minutes}} min)", {
                          minutes: Math.round((config.summary_interval_secs || 7200) / 60),
                        })}
                      </p>
                    </div>
                    <div className="space-y-2">
                      <Label>{t("systemMemory.config.summaryBackend", "Summary LLM Backend")}</Label>
                      <Select
                        value={config.summary_backend_id || "__active__"}
                        onValueChange={(value) =>
                          setConfig({
                            ...config,
                            summary_backend_id: value === "__active__" ? null : value,
                          })
                        }
                      >
                        <SelectTrigger>
                          <SelectValue placeholder={t("systemMemory.config.useActive", "Use active backend")} />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="__active__">
                            {t("systemMemory.config.useActive", "Use active backend")}
                          </SelectItem>
                          {llmBackends.map((b) => (
                            <SelectItem key={b.id} value={b.id}>
                              {b.name || b.id}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      <p className="text-xs text-muted-foreground">
                        {t("systemMemory.config.summaryBackendHint", "LLM backend for chat/agent summarization")}
                      </p>
                    </div>
                  </div>
                </div>
              </div>
            </FullScreenDialogMain>
          )}
        </FullScreenDialogContent>

        <FullScreenDialogFooter>
          <Button variant="outline" onClick={() => setConfigOpen(false)}>
            {t("common.cancel", "Cancel")}
          </Button>
          <Button onClick={handleSaveConfig} disabled={configSaving}>
            {configSaving ? (
              <Loader2 className="h-4 w-4 mr-1 animate-spin" />
            ) : (
              <Save className="h-4 w-4 mr-1" />
            )}
            {configSaving ? t("common.saving", "Saving...") : t("common.save", "Save")}
          </Button>
        </FullScreenDialogFooter>
      </FullScreenDialog>
    </div>
  )
})
