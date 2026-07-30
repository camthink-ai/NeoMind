// IM Sessions tab — rendered inside the Messages page tab content.
// Lists active IM chat sessions for the configured bridge and lets an
// operator reset (clear) the conversation for a given chat.
//
// M2a: Telegram is the only supported IM platform; the bridge id is the
// platform string itself ("telegram"), so at most one bridge exists.

import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Send, RotateCcw } from 'lucide-react'
import { ResponsiveTable, LoadingState } from '@/components/shared'
import { confirm } from '@/hooks/use-confirm'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { notifySuccess } from '@/lib/notify'
import { api, type ImBridge, type ImSession } from '@/lib/api'
import { formatTimestamp } from '@/lib/utils/format'

export function ImSessionsTab() {
  const { t } = useTranslation()
  const { handleError } = useErrorHandler()

  const [loading, setLoading] = useState(true)
  const [bridge, setBridge] = useState<ImBridge | null>(null)
  const [sessions, setSessions] = useState<ImSession[]>([])
  // chat_id currently being reset (disables all reset buttons + shows feedback)
  const [resetting, setResetting] = useState<string | null>(null)

  // Single load cycle: bridge discovery + session fetch together, so there is
  // no intermediate render that would flash the "no sessions" empty state.
  const loadAll = useCallback(async () => {
    setLoading(true)
    try {
      const res = await api.listImBridges()
      const b = (res.bridges || [])[0] ?? null
      setBridge(b)
      if (b) {
        const sres = await api.listImSessions(b.id)
        setSessions(sres.sessions || [])
      } else {
        setSessions([])
      }
    } catch (error) {
      handleError(error, { operation: 'Load IM sessions', showToast: false })
      setBridge(null)
      setSessions([])
    } finally {
      setLoading(false)
    }
  }, [handleError])

  useEffect(() => {
    loadAll()
  }, [loadAll])

  const handleReset = async (chatId: string) => {
    if (!bridge) return
    const confirmed = await confirm({
      title: t('messages.im.reset', 'Reset Session'),
      description: t(
        'messages.im.confirmReset',
        'Reset the conversation for chat {{chatId}}? This clears the bound agent session.',
        { chatId },
      ),
      confirmText: t('messages.im.reset', 'Reset'),
      cancelText: t('cancel'),
      variant: 'destructive',
    })
    if (!confirmed) return

    setResetting(chatId)
    try {
      await api.resetImSession(bridge.id, chatId)
      notifySuccess(t('messages.im.resetSuccess', 'Session reset'))
      await loadAll()
    } catch (error) {
      handleError(error, { operation: 'Reset IM session' })
    } finally {
      setResetting(null)
    }
  }

  // Initial load: bridge unknown yet → page skeleton.
  if (loading && !bridge) {
    return <LoadingState variant="page" text={t('loading', 'Loading...')} />
  }

  return (
    <>
      <ResponsiveTable
          columns={[
            {
              key: 'chat_id',
              label: (
                <div className="flex items-center gap-2">
                  <Send className="h-4 w-4" />
                  {t('messages.im.chatId', 'Chat ID')}
                </div>
              ),
            },
            {
              key: 'bound_agent_id',
              label: t('messages.im.boundAgent', 'Agent'),
            },
            {
              key: 'last_active',
              label: t('messages.im.lastActive', 'Last Active'),
              width: 'w-[180px]',
            },
          ]}
          data={sessions as unknown as Record<string, unknown>[]}
          rowKey={(row) => (row as unknown as ImSession).chat_id}
          renderCell={(columnKey, rowData) => {
            const s = rowData as unknown as ImSession
            switch (columnKey) {
              case 'chat_id':
                return <code className="text-xs font-mono">{s.chat_id}</code>
              case 'bound_agent_id':
                return s.bound_agent_id ? (
                  <span className="text-sm font-medium">{s.bound_agent_id}</span>
                ) : (
                  <span className="text-sm text-muted-foreground">-</span>
                )
              case 'last_active':
                return (
                  <span className="text-xs text-muted-foreground">
                    {formatTimestamp(s.last_active, false)}
                  </span>
                )
              default:
                return null
            }
          }}
          actions={[
            {
              label: t('messages.im.reset', 'Reset'),
              icon: <RotateCcw className="h-4 w-4" />,
              variant: 'destructive',
              disabled: resetting !== null,
              onClick: (rowData) => {
                const s = rowData as unknown as ImSession
                handleReset(s.chat_id)
              },
            },
          ]}
          loading={loading}
        />
    </>
  )
}

export default ImSessionsTab
