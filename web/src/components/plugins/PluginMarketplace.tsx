/**
 * Plugin Marketplace Component
 *
 * Displays available plugins that can be installed or configured.
 * Supports:
 * - Featured plugins
 * - Category browsing
 * - Plugin search
 * - One-click installation (for future marketplace integration)
 */

import { useState, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import {
  Tabs,
  TabsList,
  TabsTrigger,
  TabsContent,
} from '@/components/ui/tabs'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  CardFooter,
} from '@/components/ui/card'
import {
  BrainCircuit,
  Network,
  Server,
  Wifi,
  Zap,
  Sparkles,
  Gem,
  Home,
  Search,
  Download,
  ExternalLink,
  Star,
  Package,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import type { PluginCategory } from '@/types/plugin-schema'

// ============================================================================
// Types
// ============================================================================

export interface MarketplacePlugin {
  id: string
  name: string
  description: string
  longDescription?: string
  version: string
  author: string
  category: PluginCategory
  icon: string
  downloads?: number
  rating?: number
  featured?: boolean
  official?: boolean
  repository?: string
  homepage?: string
  config?: {
    type: 'builtin' | 'dynamic' | 'remote'
    downloadUrl?: string
    file?: string
    setupGuide?: string
  }
  tags: string[]
}

interface PluginMarketplaceProps {
  onInstall?: (pluginId: string) => void
  compact?: boolean
}

// ============================================================================
// Mock Marketplace Data
// ============================================================================

const MARKETPLACE_PLUGINS: MarketplacePlugin[] = [
  // AI / LLM Backends
  {
    id: 'ollama-llm',
    name: 'Ollama LLM Backend',
    description: '本地 LLM 推理引擎，支持多种开源模型',
    longDescription: '集成 Ollama 作为 LLM 后端，支持 Llama、Qwen、Mistral 等多种开源模型。适用于边缘部署和离线场景。',
    version: '1.0.0',
    author: 'NeoTalk',
    category: 'ai',
    icon: 'BrainCircuit',
    downloads: 1250,
    rating: 4.8,
    featured: true,
    official: true,
    config: { type: 'builtin' },
    tags: ['llm', 'ai', 'local', 'offline'],
  },
  {
    id: 'openai-llm',
    name: 'OpenAI LLM Backend',
    description: 'OpenAI API 集成，支持 GPT-4、GPT-3.5 等模型',
    longDescription: '通过 OpenAI API 使用 GPT-4、GPT-3.5 Turbo 等模型。需要 API Key。',
    version: '1.0.0',
    author: 'NeoTalk',
    category: 'ai',
    icon: 'Sparkles',
    downloads: 890,
    rating: 4.7,
    official: true,
    config: { type: 'builtin' },
    tags: ['llm', 'ai', 'cloud', 'gpt'],
  },
  {
    id: 'anthropic-llm',
    name: 'Anthropic Claude',
    description: 'Claude AI 模型集成，支持长上下文对话',
    longDescription: '集成 Anthropic Claude API，支持 Claude 3 Opus、Sonnet、Haiku 模型。',
    version: '1.0.0',
    author: 'NeoTalk',
    category: 'ai',
    icon: 'Gem',
    downloads: 650,
    rating: 4.9,
    config: { type: 'builtin' },
    tags: ['llm', 'ai', 'cloud', 'claude'],
  },

  // Device Adapters
  {
    id: 'mqtt-adapter',
    name: 'MQTT Broker Adapter',
    description: 'MQTT 消息代理，支持设备连接和数据传输',
    longDescription: '内置 MQTT Broker 或连接外部 MQTT 服务器，支持设备状态订阅和命令发送。',
    version: '1.0.0',
    author: 'NeoTalk',
    category: 'devices',
    icon: 'Network',
    downloads: 2100,
    rating: 4.9,
    featured: true,
    official: true,
    config: { type: 'builtin' },
    tags: ['mqtt', 'iot', 'broker'],
  },

  // Notification Channels
  {
    id: 'email-notify',
    name: 'Email Notification',
    description: '邮件通知通道，支持 SMTP 协议',
    version: '1.0.0',
    author: 'NeoTalk',
    category: 'notify',
    icon: 'Wifi',
    downloads: 780,
    rating: 4.6,
    official: true,
    config: { type: 'builtin' },
    tags: ['email', 'smtp', 'notification'],
  },
  {
    id: 'webhook-notify',
    name: 'Webhook Notification',
    description: 'Webhook 回调通知，支持自定义 HTTP 端点',
    version: '1.0.0',
    author: 'NeoTalk',
    category: 'notify',
    icon: 'Zap',
    downloads: 560,
    rating: 4.4,
    config: { type: 'builtin' },
    tags: ['webhook', 'http', 'notification'],
  },

  // Storage
  {
    id: 'redb-storage',
    name: 'Redb Storage',
    description: '嵌入式键值存储，高性能数据持久化',
    version: '1.0.0',
    author: 'NeoTalk',
    category: 'storage',
    icon: 'Server',
    downloads: 1100,
    rating: 4.7,
    featured: true,
    official: true,
    config: { type: 'builtin' },
    tags: ['storage', 'database', 'embedded'],
  },

  // Tools
  {
    id: 'example-tool',
    name: 'Example Plugin',
    description: '示例插件，展示如何开发自定义插件',
    longDescription: '一个完整的插件开发示例，包含 echo、reverse、uppercase 等命令。适合学习插件开发。',
    version: '0.1.0',
    author: 'NeoTalk Contributors',
    category: 'tools',
    icon: 'Package',
    downloads: 320,
    rating: 4.3,
    config: {
      type: 'dynamic',
      setupGuide: 'https://github.com/neotalk/neotalk/tree/main/examples/example-plugin',
    },
    tags: ['example', 'tutorial'],
  },
]

// ============================================================================
// Icon Mapping
// ============================================================================

const ICONS: Record<string, React.ComponentType<{ className?: string }>> = {
  BrainCircuit,
  Network,
  Server,
  Wifi,
  Zap,
  Sparkles,
  Gem,
  Home,
  Package,
}

function getPluginIcon(iconName: string) {
  return ICONS[iconName] || Package
}

// ============================================================================
// Plugin Card Component
// ============================================================================

interface MarketplacePluginCardProps {
  plugin: MarketplacePlugin
  onInstall?: (pluginId: string) => void
  compact?: boolean
}

function MarketplacePluginCard({ plugin, onInstall, compact = false }: MarketplacePluginCardProps) {
  const { t } = useTranslation('plugins')
  const Icon = getPluginIcon(plugin.icon)

  if (compact) {
    return (
      <Card className="hover:shadow-md transition-shadow">
        <CardContent className="p-4">
          <div className="flex items-center gap-3">
            <div className="flex items-center justify-center w-10 h-10 rounded-lg bg-primary/10">
              <Icon className="h-5 w-5 text-primary" />
            </div>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <h4 className="font-medium truncate">{plugin.name}</h4>
                {plugin.official && (
                  <Badge variant="outline" className="text-xs">Official</Badge>
                )}
              </div>
              <p className="text-xs text-muted-foreground truncate">{plugin.description}</p>
            </div>
            <Button
              size="sm"
              variant="outline"
              onClick={() => onInstall?.(plugin.id)}
            >
              {t('install')}
            </Button>
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card className="hover:shadow-lg transition-all overflow-hidden">
      <CardHeader className="pb-4">
        <div className="flex items-start gap-4">
          <div className={cn(
            "flex items-center justify-center w-16 h-16 rounded-xl shrink-0",
            plugin.category === 'ai' && "bg-purple-100 text-purple-700 dark:bg-purple-900/20 dark:text-purple-400",
            plugin.category === 'devices' && "bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400",
            plugin.category === 'storage' && "bg-amber-100 text-amber-700 dark:bg-amber-900/20 dark:text-amber-400",
            plugin.category === 'notify' && "bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400",
            plugin.category === 'tools' && "bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400",
          )}>
            <Icon className="h-8 w-8" />
          </div>
          <CardTitle className="text-lg pt-3">{plugin.name}</CardTitle>
        </div>
        {(plugin.featured || plugin.official) && (
          <div className="flex items-center gap-2 mt-2">
            {plugin.featured && (
              <Badge variant="default" className="text-xs">
                <Star className="h-3 w-3 mr-1" />
                {t('featured')}
              </Badge>
            )}
            {plugin.official && (
              <Badge variant="outline" className="text-xs">
                {t('official')}
              </Badge>
            )}
          </div>
        )}
        <div className="flex items-center gap-3 text-sm text-muted-foreground">
          <span>{plugin.author}</span>
          <span>·</span>
          <span>v{plugin.version}</span>
        </div>
      </CardHeader>

      <CardContent className="space-y-4">
        <CardDescription className="text-sm leading-relaxed">
          {plugin.description}
        </CardDescription>

        {/* Tags */}
        {plugin.tags && plugin.tags.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {plugin.tags.slice(0, 3).map(tag => (
              <Badge key={tag} variant="secondary" className="text-xs font-normal">
                {tag}
              </Badge>
            ))}
          </div>
        )}
      </CardContent>

      <CardFooter className="flex items-center justify-between gap-4 py-4 border-t">
        {plugin.repository && (
          <a
            href={plugin.repository}
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm text-muted-foreground hover:text-primary flex items-center gap-1"
          >
            <ExternalLink className="h-4 w-4" />
            {t('source')}
          </a>
        )}
        <Button
          size="sm"
          onClick={() => onInstall?.(plugin.id)}
          className="ml-auto"
        >
          {plugin.config?.type === 'builtin' ? t('plugins:configure') : (
            <>
              <Download className="h-4 w-4 mr-1" />
              {t('install')}
            </>
          )}
        </Button>
      </CardFooter>
    </Card>
  )
}

// ============================================================================
// Main Marketplace Component
// ============================================================================

export function PluginMarketplace({ onInstall, compact = false }: PluginMarketplaceProps) {
  const { t } = useTranslation(['common', 'plugins'])
  const [searchQuery, setSearchQuery] = useState('')
  const [activeTab, setActiveTab] = useState<PluginCategory | 'featured'>('featured')

  // Filter plugins
  const filteredPlugins = useMemo(() => {
    let result = MARKETPLACE_PLUGINS

    // Category filter
    if (activeTab !== 'featured' && activeTab !== 'all') {
      result = result.filter(p => p.category === activeTab)
    } else if (activeTab === 'featured') {
      result = result.filter(p => p.featured)
    }

    // Search filter
    if (searchQuery) {
      const q = searchQuery.toLowerCase()
      result = result.filter(p =>
        p.name.toLowerCase().includes(q) ||
        p.description.toLowerCase().includes(q) ||
        p.tags.some(tag => tag.toLowerCase().includes(q))
      )
    }

    return result
  }, [activeTab, searchQuery])

  // Plugin count by category
  const categoryCounts: Record<string, number> = {
    featured: MARKETPLACE_PLUGINS.filter(p => p.featured).length,
    all: MARKETPLACE_PLUGINS.length,
    ai: MARKETPLACE_PLUGINS.filter(p => p.category === 'ai').length,
    devices: MARKETPLACE_PLUGINS.filter(p => p.category === 'devices').length,
    storage: MARKETPLACE_PLUGINS.filter(p => p.category === 'storage').length,
    notify: MARKETPLACE_PLUGINS.filter(p => p.category === 'notify').length,
    tools: MARKETPLACE_PLUGINS.filter(p => p.category === 'tools').length,
  }

  return (
    <div className="space-y-6">
      {/* Search */}
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input
          placeholder={t('plugins:searchMarketplace')}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="pl-10"
        />
      </div>

      {/* Category Tabs */}
      <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as PluginCategory | 'featured')}>
        <TabsList className="flex-wrap">
          <TabsTrigger value="featured">
            {t('plugins:featured')} ({categoryCounts.featured})
          </TabsTrigger>
          <TabsTrigger value="all">
            {t('plugins:all')} ({categoryCounts.all})
          </TabsTrigger>
          <TabsTrigger value="ai">
            AI ({categoryCounts.ai})
          </TabsTrigger>
          <TabsTrigger value="devices">
            {t('plugins:categories.devices')} ({categoryCounts.devices})
          </TabsTrigger>
          <TabsTrigger value="storage">
            {t('plugins:categories.storage')} ({categoryCounts.storage})
          </TabsTrigger>
          <TabsTrigger value="notify">
            {t('plugins:categories.notify')} ({categoryCounts.notify})
          </TabsTrigger>
          <TabsTrigger value="tools">
            {t('plugins:categories.tools')} ({categoryCounts.tools})
          </TabsTrigger>
        </TabsList>

        {/* Plugin Grid */}
        {['featured', 'all', 'ai', 'devices', 'storage', 'notify', 'tools'].map(tab => (
          <TabsContent key={tab} value={tab} className="mt-6">
            {filteredPlugins.length === 0 ? (
              <div className="text-center py-12 text-muted-foreground">
                <Package className="h-12 w-12 mx-auto mb-4 opacity-50" />
                <p>{t('plugins:noPluginsFound')}</p>
              </div>
            ) : (
              <div className={cn(
                "grid gap-6",
                compact ? "grid-cols-1" : "grid-cols-1 md:grid-cols-2"
              )}>
                {filteredPlugins.map(plugin => (
                  <MarketplacePluginCard
                    key={plugin.id}
                    plugin={plugin}
                    onInstall={onInstall}
                    compact={compact}
                  />
                ))}
              </div>
            )}
          </TabsContent>
        ))}
      </Tabs>
    </div>
  )
}
