import React from 'react'
import type { ComponentConfigSchema } from '@/components/dashboard/config/ComponentConfigBuilder'
import { SelectField } from '../../ConfigFieldComponents'
import type { SchemaContext, Updaters } from '../types'

export function getToggleSwitchSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource } = u
  return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label={t('visualDashboard.size')}
                    value={config.size || 'md'}
                    onChange={updateConfig('size')}
                    options={[
                      { value: 'sm', label: t('sizes.sm') },
                      { value: 'md', label: t('sizes.md') },
                      { value: 'lg', label: t('sizes.lg') },
                    ]}
                  />
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="p-3 rounded-lg bg-info-light border border-info">
                    <p className="text-sm text-info">
                      {t('visualDashboard.commandButtonHint')}
                    </p>
                  </div>
                </div>
              ),
            },
          ],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-command', 'extension-command'],
              },
            },
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="p-3 rounded-lg bg-info-light border border-info">
                    <p className="text-sm text-info">
                      <strong>{t('visualDashboard.commandInterface')}</strong><br />
                      {t('visualDashboard.commandInterfaceDesc')}
                    </p>
                  </div>
                </div>
              ),
            },
          ],
        }
}
