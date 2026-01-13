import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Zap, Workflow as WorkflowIcon, Home, Bell, ListTodo } from 'lucide-react'
import { PageLayout } from '@/components/layout/PageLayout'
import { PageTabsGrid, PageTabsContent } from '@/components/shared'
import { RulesTab } from '@/components/automation/RulesTab'
import { WorkflowsTab } from '@/components/automation/WorkflowsTab'
import { ScenariosTab } from '@/components/automation/ScenariosTab'
import { AlertsTab } from '@/components/automation/AlertsTab'
import { CommandsTab } from '@/components/automation/CommandsTab'

type ActiveTab = 'rules' | 'workflows' | 'scenarios' | 'alerts' | 'commands'

export function AutomationPage() {
  const { t } = useTranslation(['common', 'automation'])
  const [activeTab, setActiveTab] = useState<ActiveTab>('rules')

  const tabs = [
    { value: 'rules' as ActiveTab, label: t('automation:rules'), icon: <Zap className="h-4 w-4" /> },
    { value: 'workflows' as ActiveTab, label: t('automation:workflows'), icon: <WorkflowIcon className="h-4 w-4" /> },
    { value: 'scenarios' as ActiveTab, label: t('automation:scenarios'), icon: <Home className="h-4 w-4" /> },
    { value: 'alerts' as ActiveTab, label: t('automation:alerts'), icon: <Bell className="h-4 w-4" /> },
    { value: 'commands' as ActiveTab, label: t('automation:commands'), icon: <ListTodo className="h-4 w-4" /> },
  ]

  return (
    <PageLayout>
      <PageTabsGrid
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => setActiveTab(v as ActiveTab)}
        gridCols={5}
      >
        <PageTabsContent value="rules" activeTab={activeTab}>
          <RulesTab />
        </PageTabsContent>

        <PageTabsContent value="workflows" activeTab={activeTab}>
          <WorkflowsTab />
        </PageTabsContent>

        <PageTabsContent value="scenarios" activeTab={activeTab}>
          <ScenariosTab />
        </PageTabsContent>

        <PageTabsContent value="alerts" activeTab={activeTab}>
          <AlertsTab />
        </PageTabsContent>

        <PageTabsContent value="commands" activeTab={activeTab}>
          <CommandsTab />
        </PageTabsContent>
      </PageTabsGrid>
    </PageLayout>
  )
}
