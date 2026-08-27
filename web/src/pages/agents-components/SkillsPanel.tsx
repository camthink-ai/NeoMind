/**
 * SkillsPanel - Skill management panel for the AI Agent page.
 *
 * Displays user-defined skills using ResponsiveTable.
 * Unified fullscreen dialog for view/edit/create.
 */

import { useState, useEffect, useCallback, forwardRef, useImperativeHandle } from "react"
import {
  BookOpen,
  Loader2,
  Plus,

  Trash2,
  Wrench,
  Save,
  X,
  Pencil,
  Eye,
  Hash,
  Zap,
  Target,
} from "lucide-react"
import CodeMirror from "@uiw/react-codemirror"
import { ResponsiveTable, EmptyState, LoadingState } from "@/components/shared"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
} from "@/components/automation/dialog"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { useIsMobile } from "@/hooks/useMobile"
import { useTranslation } from "react-i18next"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/hooks/use-confirm"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import { fontMonoStack } from "@/design-system/tokens/typography"
import type { SkillSummary, SkillDetail } from "@/types/skill"

// Category configuration with colors matching MemoryPanel style
const categoryConfig: Record<
  string,
  {
    label: string
    icon: React.ElementType
    color: string
  }
> = {
  device: { label: "Device", icon: Zap, color: "bg-info-light text-info border-info" },
  rule: { label: "Rule", icon: Target, color: "bg-accent-purple-light text-accent-purple border-accent-purple-light" },
  agent: { label: "Agent", icon: Wrench, color: "bg-success-light text-success border-success-light" },
  message: { label: "Message", icon: BookOpen, color: "bg-accent-orange-light text-accent-orange border-accent-orange-light" },
  extension: { label: "Extension", icon: Wrench, color: "bg-accent-cyan-light text-accent-cyan border-accent-cyan-light" },
  general: { label: "General", icon: BookOpen, color: "bg-muted text-muted-foreground border-border" },
}

// Table row type
interface SkillRow {
  id: string
  name: string
  category: string
  priority: number
  keywords: string[]
  body_length: number
}

// Default new skill template
const defaultTemplate = `---
id: my-skill
name: My Skill
category: general
priority: 50
token_budget: 500
triggers:
  keywords: [example keyword]
  tool_target:
    tool: example
    actions: [action1]
anti_triggers:
  keywords: []
---

# My Skill

Step-by-step guide here.
`

export interface SkillsPanelHandle {
  openCreate: () => void
  reload: () => void
}

export interface SkillsPanelProps {
  onPaginationChange?: (info: { total: number; pageSize: number; currentPage: number; onPageChange: (page: number) => void; loading: boolean }) => void
}

export const SkillsPanel = forwardRef<SkillsPanelHandle, SkillsPanelProps>(function SkillsPanel(
  { onPaginationChange },
  ref
) {
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
  const { t } = useTranslation("agents")
  const isMobile = useIsMobile()

  // Data state
  const [skills, setSkills] = useState<SkillSummary[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [currentPage, setCurrentPage] = useState(1)
  const [pageSize, setPageSize] = useState(10)

  // Unified dialog state
  const [dialogOpen, setDialogOpen] = useState(false)
  const [dialogMode, setDialogMode] = useState<"view" | "create" | "edit">("view")
  const [dialogContent, setDialogContent] = useState(defaultTemplate)
  const [dialogSkill, setDialogSkill] = useState<SkillDetail | null>(null)
  const [dialogLoading, setDialogLoading] = useState(false)
  const [saving, setSaving] = useState(false)

  // Check dark mode
  const isDark =
    typeof document !== "undefined" &&
    (document.documentElement.getAttribute("data-theme") === "dark" ||
      document.documentElement.classList.contains("dark"))

  // Load skills list
  const loadSkills = useCallback(async () => {
    setLoading(true)
    try {
      const res = await api.listSkills(currentPage, pageSize)
      if (isMobile && currentPage > 1) {
        // Mobile infinite scroll: append new items (dedupe by id) so the user
        // can scroll back up to see previous pages — same pattern as
        // messages.tsx and data-explorer.tsx.
        setSkills((prev) => {
          const existingIds = new Set(prev.map((s) => s.id))
          const unique = res.skills.filter((s) => !existingIds.has(s.id))
          return [...prev, ...unique]
        })
      } else {
        setSkills(res.skills)
      }
      setTotal(res.total)
    } catch (e) {
      handleError(e, { operation: "Load skills", showToast: false })
    } finally {
      setLoading(false)
    }
  }, [handleError, currentPage, pageSize, isMobile])

  useEffect(() => {
    loadSkills()
  }, [loadSkills])

  // Page change handler: immediately set loading so skeleton shows without waiting for useEffect.
  // Desktop pagination replaces data → scroll to top so the user starts at the top of the new
  // page instead of being stuck at the old scroll position (near the bottom where the pagination
  // button was clicked). Mobile uses infinite scroll that appends — never reset scroll there.
  const handlePageChange = useCallback((page: number) => {
    setCurrentPage(page)
    setLoading(true)
    if (!isMobile) {
      const scrollContainer = document.querySelector('[data-page-scroll-container]')
      if (scrollContainer) scrollContainer.scrollTo({ top: 0 })
    }
  }, [isMobile])

  // Notify parent of pagination state changes
  useEffect(() => {
    onPaginationChange?.({ total, pageSize, currentPage, onPageChange: handlePageChange, loading })
  }, [total, pageSize, currentPage, loading, onPaginationChange, handlePageChange])

  // Helper: reconstruct full .md file content from detail
  const reconstructContent = (detail: SkillDetail): string => {
    const yamlSafeStr = (s: string) => (/[,:{}[\]&*?|>!%@"`]/.test(s) ? `"${s.replace(/"/g, '\\"')}"` : s)
    const yamlSafeArr = (arr: string[]) => arr.length === 0
      ? "[]"
      : `[${arr.map((k) => yamlSafeStr(k)).join(", ")}]`

    const frontmatter = `---
id: ${detail.id}
name: ${yamlSafeStr(detail.name)}
category: ${detail.category}
priority: ${detail.priority}
token_budget: ${detail.token_budget}
triggers:
  keywords: ${yamlSafeArr(detail.keywords)}
  tool_target:
${detail.tool_targets.map((t) => `    tool: ${t.tool}\n    actions: ${yamlSafeArr(t.actions)}`).join("\n")}
anti_triggers:
  keywords: ${yamlSafeArr(detail.anti_trigger_keywords)}
---`
    return `${frontmatter}\n\n${detail.body}`
  }

  // Open dialog in view mode
  const handleView = async (id: string) => {
    setDialogLoading(true)
    setDialogOpen(true)
    setDialogMode("view")
    try {
      const detail = await api.getSkill(id)
      setDialogSkill(detail)
      setDialogContent(reconstructContent(detail))
    } catch (e) {
      handleError(e, { operation: "Load skill detail" })
      setDialogOpen(false)
    } finally {
      setDialogLoading(false)
    }
  }

  // Open dialog in edit mode
  const handleEdit = async (id: string) => {
    setDialogLoading(true)
    setDialogOpen(true)
    setDialogMode("edit")
    try {
      const detail = await api.getSkill(id)
      setDialogSkill(detail)
      setDialogContent(reconstructContent(detail))
    } catch (e) {
      handleError(e, { operation: "Load skill for editing" })
      setDialogOpen(false)
    } finally {
      setDialogLoading(false)
    }
  }

  // Open dialog in create mode
  const handleCreate = () => {
    setDialogSkill(null)
    setDialogContent(defaultTemplate)
    setDialogMode("create")
    setDialogOpen(true)
  }

  // Expose methods to parent via ref
  useImperativeHandle(ref, () => ({
    openCreate: handleCreate,
    reload: () => {
      loadSkills()
    },
  }), [loadSkills])

  // Close dialog
  const closeDialog = () => {
    setDialogOpen(false)
  }

  // Switch from view to edit
  const switchToEdit = () => {
    if (!dialogSkill) return
    setDialogMode("edit")
  }

  // Save skill (create or update)
  const handleSave = async () => {
    setSaving(true)
    try {
      if (dialogMode === "create") {
        await api.createSkill(dialogContent)
      } else if (dialogSkill) {
        await api.updateSkill(dialogSkill.id, dialogContent)
      }
      toast({ title: dialogMode === "create" ? t("skills.skillCreated") : t("skills.skillUpdated") })
      closeDialog()
      loadSkills()
    } catch (e) {
      handleError(e, { operation: "Save skill" })
    } finally {
      setSaving(false)
    }
  }

  // Delete skill
  const handleDelete = async (id: string, name: string) => {
    const confirmed = await confirm({
      title: t("skills.deleteSkill"),
      description: t("skills.deleteConfirm", { name }),
      confirmText: t("skills.delete"),
      cancelText: t("skills.cancel"),
      variant: "destructive",
    })
    if (!confirmed) return

    try {
      await api.deleteSkill(id)
      toast({ title: t("skills.skillDeleted", { name }) })
      loadSkills()
    } catch (e) {
      handleError(e, { operation: "Delete skill" })
    }
  }

  // Table data
  const tableData: SkillRow[] = skills.map((skill) => ({
    id: skill.id,
    name: skill.name,
    category: skill.category,
    priority: skill.priority,
    keywords: skill.keywords,
    body_length: skill.body_length,
  }))

  // Dialog title
  const dialogTitle =
    dialogMode === "create"
      ? t("skills.createNewSkill")
      : dialogSkill?.name || ""

  const lineCount = dialogContent.split("\n").filter((l) => l.trim()).length

  return (
    <div className="space-y-4">
      {/* Skills table */}
      <ResponsiveTable
        columns={[
          {
            key: "name",
            label: t("skills.columnSkill"),
          },
          {
            key: "category",
            label: t("skills.columnCategory"),
            width: "w-28",
          },
          {
            key: "priority",
            label: (
              <div className="flex items-center gap-1">
                <Hash className="h-4 w-4" />
                {t("skills.columnPriority")}
              </div>
            ),
            align: "center",
            width: "w-20",
          },
          {
            key: "keywords",
            label: t("skills.columnKeywords"),
          },
          {
            key: "body_length",
            label: t("skills.columnSize"),
            align: "right",
            width: "w-24",
          },
        ]}
        data={tableData}
        rowKey={(row: SkillRow) => row.id}
        loading={loading}
        onRowClick={(row: SkillRow) => handleView(row.id)}
        renderCell={(columnKey, rowData) => {
          const row = rowData
          const catConf = categoryConfig[row.category] || categoryConfig.general
          const Icon = catConf.icon

          switch (columnKey) {
            case "name":
              // Compact header cell: icon (32px, matches other list pages) + name only.
              // Category and keywords live in their own columns so the mobile card
              // header stays a single-line anchor point, not a multi-row info dump.
              return (
                <div className="flex items-center gap-2.5">
                  <div
                    className={cn(
                      "w-8 h-8 rounded-lg flex items-center justify-center border shrink-0",
                      catConf.color
                    )}
                  >
                    <Icon className="h-4 w-4" />
                  </div>
                  <div className="font-medium text-sm truncate min-w-0">
                    {row.name}
                  </div>
                </div>
              )

            case "category":
              return (
                <Badge variant="outline" className={cn("h-6 gap-1", catConf.color)}>
                  <Icon className="h-3 w-3" />
                  {catConf.label}
                </Badge>
              )

            case "priority":
              return (
                <Badge variant="secondary" className="font-mono">
                  {row.priority}
                </Badge>
              )

            case "keywords":
              // Render up to 3 keyword tags + overflow count.
              // On mobile this lands in the card body (white bg) where the
              // muted-background tags have proper contrast — previously they
              // sat inside the bg-muted card header and were nearly invisible.
              if (row.keywords.length === 0) {
                return <span className="text-xs text-muted-foreground">—</span>
              }
              return (
                <div className="flex flex-wrap gap-1">
                  {row.keywords.slice(0, 3).map((kw, i) => (
                    <span key={i} className="text-xs bg-muted px-1.5 py-0.5 rounded">
                      {kw}
                    </span>
                  ))}
                  {row.keywords.length > 3 && (
                    <span className="text-xs text-muted-foreground self-center">
                      +{row.keywords.length - 3}
                    </span>
                  )}
                </div>
              )

            case "body_length":
              return (
                <span className="text-xs text-muted-foreground font-mono">
                  {row.body_length > 1024
                    ? `${(row.body_length / 1024).toFixed(1)} KB`
                    : `${row.body_length} B`}
                </span>
              )

            default:
              return null
          }
        }}
        actions={[
          {
            label: t("skills.actionView"),
            icon: <Eye className="h-4 w-4" />,
            onClick: (rowData) => {
              const row = rowData
              handleView(row.id)
            },
          },
          {
            label: t("skills.actionEdit"),
            icon: <Pencil className="h-4 w-4" />,
            onClick: (rowData) => {
              const row = rowData
              handleEdit(row.id)
            },
          },
          {
            label: t("skills.actionDelete"),
            icon: <Trash2 className="h-4 w-4" />,
            onClick: (rowData) => {
              const row = rowData
              handleDelete(row.id, row.name)
            },
          },
        ]}
        emptyState={
          <EmptyState
            icon={<BookOpen className="h-12 w-12" />}
            title={t("skills.emptyTitle")}
            description={t("skills.emptyDesc")}
          />
        }
      />

      {/* Unified Dialog (view / edit / create) */}
      <FullScreenDialog open={dialogOpen} onOpenChange={(open) => { if (!open) closeDialog() }}>
        <FullScreenDialogHeader
          icon={
            dialogMode === "create" ? (
              <Plus className="h-5 w-5" />
            ) : (
              <BookOpen className="h-5 w-5" />
            )
          }
          iconBg={
            dialogSkill
              ? categoryConfig[dialogSkill.category]?.color || "bg-muted"
              : "bg-success-light"
          }
          iconColor={dialogMode === "create" ? "text-success" : undefined}
          title={dialogTitle}
          onClose={closeDialog}
        />

        <FullScreenDialogContent>
          {dialogLoading ? (
            <LoadingState size="lg" className="h-full" />
          ) : (
            <div className="w-full h-full overflow-hidden">
              <CodeMirror
                value={dialogContent}
                height="100%"
                onChange={dialogMode !== "view" ? (val) => setDialogContent(val) : undefined}
                editable={dialogMode !== "view"}
                theme={isDark ? "dark" : "light"}
                style={{
                  fontSize: "14px",
                  fontFamily: fontMonoStack,
                  height: "100%",
                  width: "100%",
                }}
              />
            </div>
          )}
        </FullScreenDialogContent>

        <FullScreenDialogFooter>
          <div className="flex items-center justify-between w-full">
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">
                {t("skills.lines", { count: lineCount })}
              </span>
            </div>
            <div className="flex items-center gap-2">
              {dialogMode === "view" && (
                <Button onClick={switchToEdit}>
                  <Pencil className="h-4 w-4 mr-1" />
                  {t("skills.btnEdit")}
                </Button>
              )}
              {dialogMode !== "view" && (
                <>
                  <Button variant="outline" onClick={closeDialog} disabled={saving}>
                    <X className="h-4 w-4 mr-1" />
                    {t("skills.btnCancel")}
                  </Button>
                  <Button onClick={handleSave} disabled={saving}>
                    {saving ? (
                      <Loader2 className="h-4 w-4 mr-1 animate-spin" />
                    ) : (
                      <Save className="h-4 w-4 mr-1" />
                    )}
                    {saving
                      ? t("skills.btnSaving")
                      : dialogMode === "create"
                        ? t("skills.btnCreate")
                        : t("skills.btnSave")}
                  </Button>
                </>
              )}
            </div>
          </div>
        </FullScreenDialogFooter>
      </FullScreenDialog>
    </div>
  )
})
