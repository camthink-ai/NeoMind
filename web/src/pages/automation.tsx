import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Zap, Workflow as WorkflowIcon, Home } from 'lucide-react'
import { PageLayout } from '@/components/layout/PageLayout'
import { PageTabsGrid, PageTabsContent } from '@/components/shared'
import { RulesTab } from '@/components/automation/RulesTab'
import { WorkflowsTab } from '@/components/automation/WorkflowsTab'
import { ScenariosTab } from '@/components/automation/ScenariosTab'

type ActiveTab = 'rules' | 'workflows' | 'scenarios'

export function AutomationPage() {
  const { t } = useTranslation(['common', 'automation'])
  const [activeTab, setActiveTab] = useState<ActiveTab>('rules')

  const tabs = [
    { value: 'rules' as ActiveTab, label: t('automation:rules'), icon: <Zap className="h-4 w-4" /> },
    { value: 'workflows' as ActiveTab, label: t('automation:workflows'), icon: <WorkflowIcon className="h-4 w-4" /> },
    { value: 'scenarios' as ActiveTab, label: t('automation:scenarios'), icon: <Home className="h-4 w-4" /> },
  ]

  return (
    <PageLayout>
      <PageTabsGrid
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => setActiveTab(v as ActiveTab)}
        gridCols={3}
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
      </PageTabsGrid>
    </PageLayout>
  )
}
