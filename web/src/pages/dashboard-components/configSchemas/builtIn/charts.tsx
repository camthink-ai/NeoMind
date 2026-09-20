import React from 'react'
import { chartColorsHex } from '@/design-system/tokens/color'
import { Field } from '@/components/ui/field'
import { Label } from '@/components/ui/label'
import { Checkbox } from '@/components/ui/checkbox'
import { ColorPicker } from '@/components/ui/color-picker'
import { DataMappingConfig } from '@/components/dashboard/config/UIConfigSections'
import type { TimeSeriesMappingConfig } from '@/lib/dataMapping'
import type { ComponentConfigSchema } from '@/components/dashboard/config/ComponentConfigBuilder'
import { SelectField } from '../../ConfigFieldComponents'
import type { SchemaContext, Updaters } from '../types'

export function getLineChartSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource, updateDataMapping: _updateDataMapping } = u
  return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <ColorPicker
                    value={config.color || chartColorsHex[0]}
                    onChange={(color) => updateConfig('color')(color)}
                    label={t('visualDashboard.lineColor')}
                    presets="primary"
                  />

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

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.smooth ?? true}
                        onCheckedChange={(checked) => updateConfig('smooth')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.smoothCurve')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.fillArea ?? false}
                        onCheckedChange={(checked) => updateConfig('fillArea')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.fillArea')}</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showGrid ?? true}
                        onCheckedChange={(checked) => updateConfig('showGrid')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showGrid')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showLegend ?? false}
                        onCheckedChange={(checked) => updateConfig('showLegend')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showLegend')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showTooltip ?? true}
                        onCheckedChange={(checked) => updateConfig('showTooltip')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showTooltip')}</span>
                    </label>
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
                allowedTypes: ['device-metric', 'system', 'extension', 'transform'],
                multiple: true,
                maxSources: 5,
              },
            },
          ],
        }
}

export function getAreaChartSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource, updateDataMapping } = u
  return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <ColorPicker
                    value={config.color || chartColorsHex[0]}
                    onChange={(color) => updateConfig('color')(color)}
                    label={t('visualDashboard.areaColor')}
                    presets="primary"
                  />

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

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.smooth ?? true}
                        onCheckedChange={(checked) => updateConfig('smooth')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.smoothCurve')}</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showGrid ?? true}
                        onCheckedChange={(checked) => updateConfig('showGrid')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showGrid')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showLegend ?? false}
                        onCheckedChange={(checked) => updateConfig('showLegend')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showLegend')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showTooltip ?? true}
                        onCheckedChange={(checked) => updateConfig('showTooltip')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showTooltip')}</span>
                    </label>
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
                allowedTypes: ['device-metric', 'system', 'extension', 'transform'],
                multiple: true,
                maxSources: 5,
              },
            },
            {
              type: 'custom' as const,
              render: () => (
                <DataMappingConfig
                  dataMapping={config.dataMapping as TimeSeriesMappingConfig}
                  onChange={updateDataMapping}
                  mappingType="time-series"
                  label={t('visualDashboard.dataMappingConfig')}
                  readonly={false}
                />
              ),
            },
          ],
        }
}

export function getBarChartSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource } = u
  return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <ColorPicker
                    value={config.color || chartColorsHex[0]}
                    onChange={(color) => updateConfig('color')(color)}
                    label={t('visualDashboard.barColor')}
                    presets="primary"
                  />

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

                  <SelectField
                    label={t('visualDashboard.layout')}
                    value={config.layout || 'vertical'}
                    onChange={updateConfig('layout')}
                    options={[
                      { value: 'vertical', label: t('visualDashboard.vertical') },
                      { value: 'horizontal', label: t('visualDashboard.horizontal') },
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
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.stacked ?? false}
                        onCheckedChange={(checked) => updateConfig('stacked')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.stacked')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showGrid ?? true}
                        onCheckedChange={(checked) => updateConfig('showGrid')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showGrid')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showLegend ?? false}
                        onCheckedChange={(checked) => updateConfig('showLegend')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showLegend')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showTooltip ?? true}
                        onCheckedChange={(checked) => updateConfig('showTooltip')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showTooltip')}</span>
                    </label>
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
                allowedTypes: ['device-metric', 'system', 'extension', 'transform'],
                multiple: true,
                maxSources: 3,
              },
            },
          ],
        }
}

export function getPieChartSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
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

                  <SelectField
                    label={t('visualDashboard.type')}
                    value={config.variant || 'donut'}
                    onChange={updateConfig('variant')}
                    options={[
                      { value: 'pie', label: t('visualDashboard.pie') },
                      { value: 'donut', label: t('visualDashboard.donut') },
                    ]}
                  />

                  {config.variant === 'donut' && (
                    <Field>
                      <Label>{t('visualDashboard.innerRadius')}</Label>
                      <input
                        type="text"
                        value={config.innerRadius || '60%'}
                        onChange={(e) => updateConfig('innerRadius')(e.target.value)}
                        placeholder="60% or 60"
                        className="w-full h-9 px-3 rounded-md border border-input bg-card text-sm"
                      />
                    </Field>
                  )}

                  <Field>
                    <Label>{t('visualDashboard.outerRadius')}</Label>
                    <input
                      type="text"
                      value={config.outerRadius || '80%'}
                      onChange={(e) => updateConfig('outerRadius')(e.target.value)}
                      placeholder="80% or 80"
                      className="w-full h-9 px-3 rounded-md border border-input bg-card text-sm"
                    />
                  </Field>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showLegend ?? false}
                        onCheckedChange={(checked) => updateConfig('showLegend')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showLegend')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showTooltip ?? true}
                        onCheckedChange={(checked) => updateConfig('showTooltip')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showTooltip')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showLabels ?? false}
                        onCheckedChange={(checked) => updateConfig('showLabels')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showLabel')}</span>
                    </label>
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
                allowedTypes: ['device-metric', 'system', 'extension', 'transform'],
              },
            },
          ],
        }
}
