/**
 * ToolsPanel - Read-only catalog of the server's ToolRegistry.
 *
 * Tools are registered at the server level (built-in, extension, or custom),
 * so this panel lives at the page level alongside Memory and Skills — not
 * inside per-agent detail views. Mirror SkillsPanel layout (ResponsiveTable +
 * FullScreenDialog detail), but read-only: no create/edit/delete.
 */

import { useState, useEffect, useCallback, useMemo } from "react"
import { Wrench, Eye, Boxes, Layers } from "lucide-react"
import { ResponsiveTable, EmptyState } from "@/components/shared"
import { Badge } from "@/components/ui/badge"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
} from "@/components/automation/dialog"
import { Button } from "@/components/ui/button"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { useTranslation } from "react-i18next"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import type { AgentToolCatalogItem } from "@/types"

// Source configuration: icon + color + label key, same color scheme as
// SkillsPanel categoryConfig for visual consistency.
// Only two real sources exist in the system:
//   - "built-in": tools compiled into the Rust binary (shell, web_fetch, etc.)
//   - "extension": tools registered by .nep extension packages
const sourceConfig: Record<
  string,
  { icon: React.ElementType; color: string; labelKey: string }
> = {
  "built-in": { icon: Boxes, color: "bg-info-light text-info border-info", labelKey: "agents:detail.toolsSourceBuiltIn" },
  extension: { icon: Layers, color: "bg-accent-purple-light text-accent-purple border-accent-purple-light", labelKey: "agents:detail.toolsSourceExtension" },
}

function resolveSource(source: string) {
  return sourceConfig[source] || sourceConfig["built-in"]
}

function countParameters(params: Record<string, unknown> | undefined): number {
  const props = params?.properties
  if (props && typeof props === "object" && props !== null) {
    return Object.keys(props as Record<string, unknown>).length
  }
  return 0
}

/** Extract parameter names from a JSON Schema, respecting `required` order first. */
function parameterNames(params: Record<string, unknown> | undefined): string[] {
  const props = params?.properties
  if (!props || typeof props !== "object" || props === null) return []
  const all = Object.keys(props as Record<string, unknown>)
  const required = (params?.required)
  if (Array.isArray(required)) {
    const reqSet = new Set(required as string[])
    return [...all].sort((a, b) => {
      const ra = reqSet.has(a) ? 0 : 1
      const rb = reqSet.has(b) ? 0 : 1
      return ra - rb
    })
  }
  return all
}

export type ToolSourceFilter = 'all' | 'built-in' | 'extension'

export interface ToolsPanelProps {
  /** Lifted pagination state for PageLayout footer, mirrors SkillsPanel pattern. */
  onPaginationChange?: (info: {
    total: number
    pageSize: number
    currentPage: number
    onPageChange: (page: number) => void
    loading: boolean
  }) => void
  /** Optional search query (debounced upstream, applied client-side). */
  searchQuery?: string
  /** Source filter value (controlled from page-level dropdown next to search). */
  sourceFilter?: ToolSourceFilter
}

const TOOLS_PAGE_SIZE = 10
interface ToolRow {
  name: string
  description: string
  source: string
  category: string
  namespace: string | null
  version: string | null
  deprecated: boolean
  disabled: boolean
  param_count: number
  param_names: string[]
  required_params: string[]
  parameters?: Record<string, unknown>
}

export function ToolsPanel({ onPaginationChange, searchQuery = "", sourceFilter = 'all' }: ToolsPanelProps = {}) {
  const { handleError } = useErrorHandler()
  const { t } = useTranslation(["agents", "common"])

  const [tools, setTools] = useState<AgentToolCatalogItem[]>([])
  const [loading, setLoading] = useState(true)
  const [currentPage, setCurrentPage] = useState(1)

  // Detail dialog state
  const [dialogOpen, setDialogOpen] = useState(false)
  const [dialogTool, setDialogTool] = useState<AgentToolCatalogItem | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const data = await api.getAgentTools()
      setTools(data.tools || [])
    } catch (e) {
      handleError(e, { operation: "Load agent tools catalog", showToast: false })
    } finally {
      setLoading(false)
    }
  }, [handleError])

  useEffect(() => {
    load()
  }, [load])

  // Client-side search + source filter (search query comes from parent header).
  const filteredTools = useMemo(() => {
    const q = searchQuery.trim().toLowerCase()
    return tools.filter((tool) => {
      if (sourceFilter !== 'all' && tool.source !== sourceFilter) return false
      if (!q) return true
      return (
        tool.name.toLowerCase().includes(q) ||
        (tool.description ?? "").toLowerCase().includes(q) ||
        (tool.namespace ?? "").toLowerCase().includes(q) ||
        tool.source.toLowerCase().includes(q) ||
        tool.category.toLowerCase().includes(q)
      )
    })
  }, [tools, searchQuery, sourceFilter])

  // Reset to page 1 whenever the filter changes to avoid landing on an
  // out-of-range page when the result set shrinks.
  useEffect(() => {
    setCurrentPage(1)
  }, [searchQuery, sourceFilter])

  // Pagination: client-side slice of the filtered list.
  const total = filteredTools.length
  const pageCount = Math.max(1, Math.ceil(total / TOOLS_PAGE_SIZE))
  const safePage = Math.min(currentPage, pageCount)
  const startIdx = (safePage - 1) * TOOLS_PAGE_SIZE
  const pagedTools = filteredTools.slice(startIdx, startIdx + TOOLS_PAGE_SIZE)

  // Notify parent of pagination state for PageLayout footer (mirror SkillsPanel).
  useEffect(() => {
    onPaginationChange?.({
      total,
      pageSize: TOOLS_PAGE_SIZE,
      currentPage: safePage,
      onPageChange: (page: number) => setCurrentPage(page),
      loading,
    })
  }, [total, safePage, loading, onPaginationChange])

  const handleView = (tool: AgentToolCatalogItem) => {
    setDialogTool(tool)
    setDialogOpen(true)
  }

  const closeDialog = () => {
    setDialogOpen(false)
  }

  const tableData: ToolRow[] = pagedTools.map((tool) => {
    const paramNames = parameterNames(tool.parameters)
    const requiredRaw = tool.parameters?.required
    const required = Array.isArray(requiredRaw) ? (requiredRaw as string[]) : []
    return {
      name: tool.name,
      description: tool.description,
      source: tool.source,
      category: tool.category,
      namespace: tool.namespace ?? null,
      version: tool.version ?? null,
      deprecated: !!tool.deprecated,
      disabled: !!tool.disabled,
      param_count: paramNames.length,
      param_names: paramNames,
      required_params: required,
      parameters: tool.parameters,
    }
  })

  const dialogSource = dialogTool ? resolveSource(dialogTool.source) : null
  const dialogParamCount = dialogTool ? countParameters(dialogTool.parameters) : 0

  return (
    <div className="space-y-4">
      <ResponsiveTable
        className="table-fixed"
        columns={[
          { key: "name", label: t("agents:detail.toolsColumnName", "Tool"), width: "w-[440px]" },
          {
            key: "version",
            label: t("agents:detail.toolsColumnVersion", "Version"),
            align: "center",
            width: "w-24",
          },
          {
            key: "source",
            label: t("agents:detail.toolsColumnSource"),
            align: "center",
            width: "w-32",
          },
          {
            key: "namespace",
            label: t("agents:detail.toolsColumnNamespace", "Namespace"),
            width: "w-[160px]",
          },
          {
            key: "param_names",
            label: t("agents:detail.toolsColumnParams", "Parameters"),
          },
        ]}
        data={tableData}
        rowKey={(row: ToolRow) => row.name}
        loading={loading}
        onRowClick={(rowData) => {
          const row = rowData
          const tool = pagedTools.find((t2) => t2.name === row.name)
          if (tool) handleView(tool)
        }}
        getRowClassName={(rowData) => {
          const row = rowData
          // Locked/disabled row: muted bg tint + faded foreground text.
          // Badges with their own explicit text/bg colors (Disabled badge,
          // source badge) keep their accent so the state stays readable.
          return row.disabled ? "bg-muted-30 text-muted-foreground" : ""
        }}
        renderCell={(columnKey, rowData) => {
          const row = rowData
          const src = resolveSource(row.source)
          const SrcIcon = src.icon

          switch (columnKey) {
            case "name":
              return (
                <div className="flex items-center gap-2.5 min-w-0 w-full overflow-hidden">
                  <div
                    className={cn(
                      "w-8 h-8 rounded-lg flex items-center justify-center border shrink-0",
                      src.color
                    )}
                  >
                    <SrcIcon className="h-4 w-4" />
                  </div>
                  <div className="min-w-0 flex-1 overflow-hidden">
                    <div className="flex items-center gap-1.5 min-w-0">
                      <span className="font-mono text-sm font-medium truncate">
                        {row.name}
                      </span>
                      {row.deprecated && (
                        <span className="text-[10px] uppercase tracking-wide text-error shrink-0">
                          {t("agents:detail.toolsDeprecated")}
                        </span>
                      )}
                      {row.disabled && (
                        <span className="text-[10px] uppercase tracking-wide text-warning shrink-0 bg-warning-light border border-warning-light rounded px-1">
                          {t("agents:detail.toolsDisabled", { defaultValue: "Disabled" })}
                        </span>
                      )}
                    </div>
                    <div className="text-xs text-muted-foreground line-clamp-2 break-words overflow-hidden">
                      {row.description}
                    </div>
                  </div>
                </div>
              )

            case "source":
              return (
                <Badge variant="outline" className={cn("h-6 gap-1", src.color)}>
                  <SrcIcon className="h-3 w-3" />
                  {t(src.labelKey)}
                </Badge>
              )

            case "namespace": {
              // Extension tools: show the namespace (which extension package this
              // tool belongs to, e.g. "weather-forecast"). Built-in tools: show
              // the lowercased category as a module hint (e.g. "system").
              const value = row.namespace || (row.category && row.category !== "Unknown" ? row.category.toLowerCase() : "")
              return value ? (
                <code className="text-xs font-mono text-muted-foreground truncate block max-w-full" title={value}>
                  {value}
                </code>
              ) : (
                <span className="text-xs text-muted-foreground">—</span>
              )
            }

            case "param_names": {
              if (row.param_names.length === 0) {
                return (
                  <span className="text-xs text-muted-foreground italic">
                    {t("agents:detail.toolsNoParameters")}
                  </span>
                )
              }
              const requiredSet = new Set(row.required_params)
              return (
                <div className="flex flex-wrap gap-1">
                  {row.param_names.slice(0, 6).map((name) => (
                    <span
                      key={name}
                      className={cn(
                        "text-[11px] font-mono px-1.5 py-0.5 rounded border",
                        requiredSet.has(name)
                          ? "bg-accent-orange-light text-accent-orange border-accent-orange-light"
                          : "bg-muted text-muted-foreground border-border"
                      )}
                      title={requiredSet.has(name) ? t("agents:detail.toolsParamRequired", "required") : undefined}
                    >
                      {name}
                    </span>
                  ))}
                  {row.param_names.length > 6 && (
                    <span className="text-[11px] text-muted-foreground self-center">
                      +{row.param_names.length - 6}
                    </span>
                  )}
                </div>
              )
            }

            case "version":
              return row.version ? (
                <Badge variant="secondary" className="font-mono">
                  v{row.version}
                </Badge>
              ) : (
                <span className="text-xs text-muted-foreground">—</span>
              )

            default:
              return null
          }
        }}
        actions={[
          {
            label: t("agents:detail.toolsActionView", "View"),
            icon: <Eye className="h-4 w-4" />,
            onClick: (rowData) => {
              const row = rowData
              const tool = pagedTools.find((t2) => t2.name === row.name)
              if (tool) handleView(tool)
            },
          },
        ]}
        emptyState={
          <EmptyState
            icon={<Wrench className="h-12 w-12" />}
            title={
              searchQuery.trim()
                ? t("common:noResults", "No matching results")
                : t("agents:detail.toolsEmpty")
            }
          />
        }
      />

      {/* Detail Dialog (read-only) */}
      <FullScreenDialog open={dialogOpen} onOpenChange={(open) => { if (!open) closeDialog() }}>
        <FullScreenDialogHeader
          icon={<Wrench className="h-5 w-5" />}
          iconBg={dialogSource?.color || "bg-muted"}
          title={dialogTool?.name || ""}
          onClose={closeDialog}
        />

        <FullScreenDialogContent>
          {dialogTool && (
            <div className="w-full h-full overflow-auto px-6 py-4 space-y-5">
              {/* Disabled banner — only shown when the tool is hidden from the LLM */}
              {dialogTool.disabled && (
                <div className="rounded-md border border-warning-light bg-warning-light text-warning px-3 py-2 text-xs flex items-center gap-2">
                  <Eye className="h-3.5 w-3.5 shrink-0" />
                  <span>
                    {t("agents:detail.toolsDisabledHint", {
                      defaultValue: "This tool is hidden from the agent. Enable it on the Extensions page.",
                    })}
                  </span>
                </div>
              )}

              {/* Description */}
              <section>
                <h3 className="text-xs uppercase tracking-wide text-muted-foreground mb-1.5">
                  {t("agents:detail.toolsColumnDesc", "Description")}
                </h3>
                <p className="text-sm leading-relaxed whitespace-pre-wrap text-foreground">
                  {dialogTool.description || "—"}
                </p>
              </section>

              {/* Meta */}
              <section>
                <h3 className="text-xs uppercase tracking-wide text-muted-foreground mb-1.5">
                  {t("agents:detail.toolsMeta", "Metadata")}
                </h3>
                <div className="grid grid-cols-2 gap-2 text-xs">
                  <MetaItem label={t("agents:detail.toolsColumnSource")}>
                    {dialogSource && (
                      <Badge variant="outline" className={cn("h-5 gap-1", dialogSource.color)}>
                        <dialogSource.icon className="h-3 w-3" />
                        {t(dialogSource.labelKey)}
                      </Badge>
                    )}
                  </MetaItem>
                  <MetaItem label={t("agents:detail.toolsCategory")}>
                    <span className="text-muted-foreground">{dialogTool.category || "—"}</span>
                  </MetaItem>
                  <MetaItem label={t("agents:detail.toolsNamespace")}>
                    {dialogTool.namespace ? (
                      <code className="font-mono">{dialogTool.namespace}</code>
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </MetaItem>
                  <MetaItem label={t("agents:detail.toolsVersion")}>
                    <span className="text-muted-foreground">
                      {dialogTool.version ? `v${dialogTool.version}` : "—"}
                    </span>
                  </MetaItem>
                </div>
              </section>

              {/* Parameters JSON Schema */}
              <section>
                <h3 className="text-xs uppercase tracking-wide text-muted-foreground mb-1.5">
                  {t("agents:detail.toolsParameters")}
                  <span className="ml-1 text-muted-foreground">({dialogParamCount})</span>
                </h3>
                {dialogParamCount === 0 ? (
                  <p className="text-xs text-muted-foreground italic">
                    {t("agents:detail.toolsNoParameters")}
                  </p>
                ) : (
                  <pre className="text-xs font-mono bg-muted border rounded-md overflow-auto max-h-[60vh] p-3">
                    {JSON.stringify(dialogTool.parameters, null, 2)}
                  </pre>
                )}
              </section>
            </div>
          )}
        </FullScreenDialogContent>

        <FullScreenDialogFooter>
          <div className="flex items-center justify-between w-full">
            <p className="text-xs text-muted-foreground italic">
              {t("agents:detail.toolsReadonlyHint")}
            </p>
            <Button variant="outline" onClick={closeDialog}>
              {t("common:close", "Close")}
            </Button>
          </div>
        </FullScreenDialogFooter>
      </FullScreenDialog>
    </div>
  )
}

// ============================================================================
// Sub-components
// ============================================================================

function MetaItem({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </span>
      <div className="text-sm">{children}</div>
    </div>
  )
}
