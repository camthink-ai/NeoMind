/**
 * Shared navigation item definitions — consumed by the desktop AppSidebar
 * (and available to any nav surface). The mobile MobileNav drawer currently
 * maintains its own entry list.
 */

import {
  MessageSquare,
  Bot,
  LayoutDashboard,
  Cpu,
  Workflow,
  Database,
  Bell,
  Puzzle,
} from "lucide-react"

type PageType =
  | "dashboard"
  | "agents"
  | "visual-dashboard"
  | "devices"
  | "automation"
  | "data"
  | "messages"
  | "extensions"
  | "settings"

export interface NavItem {
  id: PageType
  path: string
  icon: React.ComponentType<{ className?: string }>
  labelKey: string
  /** Shorter label for mobile contexts (falls back to labelKey if not set) */
  mobileLabelKey?: string
}

/** Route order = display order in the sidebar's PRIMARY group */
export const navItems: NavItem[] = [
  { id: "dashboard", path: "/chat", icon: MessageSquare, labelKey: "nav.dashboard" },
  { id: "agents", path: "/agents", icon: Bot, labelKey: "nav.agents" },
  { id: "devices", path: "/devices", icon: Cpu, labelKey: "nav.devices" },
  { id: "visual-dashboard", path: "/visual-dashboard", icon: LayoutDashboard, labelKey: "nav.visual-dashboard" },
  { id: "automation", path: "/automation", icon: Workflow, labelKey: "nav.automation" },
  { id: "data", path: "/data", icon: Database, labelKey: "nav.data" },
  { id: "messages", path: "/messages", icon: Bell, labelKey: "nav.messages" },
  { id: "extensions", path: "/extensions", icon: Puzzle, labelKey: "nav.extensions" },
]

/** Active-route check, shared by sidebar and any nav surface */
export function isNavItemActive(item: NavItem, currentPath: string): boolean {
  const path = currentPath.endsWith("/") && currentPath !== "/"
    ? currentPath.slice(0, -1)
    : currentPath
  return path === item.path ||
    (item.path === "/chat" && path === "/") ||
    path.startsWith(`${item.path}/`)
}
