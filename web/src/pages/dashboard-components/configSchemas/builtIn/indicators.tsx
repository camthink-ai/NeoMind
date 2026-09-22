import React from 'react'
import { chartColorsHex } from '@/design-system/tokens/color'
import { Field } from '@/components/ui/field'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Checkbox } from '@/components/ui/checkbox'
import { ColorPicker } from '@/components/ui/color-picker'
import { IconPicker } from '@/components/ui/icon-picker'
import { EntityIconPicker } from '@/components/ui/entity-icon-picker'
import { DataMappingConfig } from '@/components/dashboard/config/UIConfigSections'
import { LEDStateRulesConfig } from '@/components/dashboard/config/LEDStateRulesConfig'
import type { StateRule } from '@/components/dashboard/generic/LEDIndicator'
import type { SingleValueMappingConfig } from '@/lib/dataMapping'
import type { ComponentConfigSchema } from '@/components/dashboard/config/ComponentConfigBuilder'
import { SelectField } from '../../ConfigFieldComponents'
import type { SchemaContext, Updaters } from '../types'

export function getValueCardSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
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
                    label={t('visualDashboard.style')}
                    value={config.variant || 'default'}
                    onChange={updateConfig('variant')}
                    options={[
                      { value: 'default', label: t('visualDashboard.default') },
                      { value: 'vertical', label: t('visualDashboard.vertical') },
                      { value: 'compact', label: t('visualDashboard.compact') },
                      { value: 'minimal', label: t('visualDashboard.minimal') },
                    ]}
                  />

                  <EntityIconPicker
                    value={config.icon || ''}
                    onChange={(icon) => updateConfig('icon')(icon)}
                    label={t('visualDashboard.icon')}
                  />

                  <SelectField
                    label={t('visualDashboard.iconType')}
                    value={config.iconType || 'entity'}
                    onChange={updateConfig('iconType')}
                    options={[
                      { value: 'entity', label: t('visualDashboard.entityIcon') },
                      { value: 'class', label: t('visualDashboard.lucideIcon') },
                    ]}
                  />

                  <ColorPicker
                    value={config.iconColor || chartColorsHex[0]}
                    onChange={(color) => updateConfig('iconColor')(color)}
                    label={t('visualDashboard.iconColor')}
                    presets="primary"
                  />

                  <ColorPicker
                    value={config.valueColor || chartColorsHex[0]}
                    onChange={(color) => updateConfig('valueColor')(color)}
                    label={t('visualDashboard.valueColor')}
                    presets="primary"
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
                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.prefix')}</Label>
                      <Input
                        value={config.prefix || ''}
                        onChange={(e) => updateConfig('prefix')(e.target.value)}
                        placeholder={t('visualDashboard.prefixPlaceholder')}
                        className="h-9"
                      />
                    </Field>

                    <Field>
                      <Label>{t('visualDashboard.unit')}</Label>
                      <Input
                        value={config.unit || ''}
                        onChange={(e) => updateConfig('unit')(e.target.value)}
                        placeholder={t('visualDashboard.unitPlaceholder')}
                        className="h-9"
                      />
                    </Field>
                  </div>

                  <Field>
                    <Label>{t('visualDashboard.description')}</Label>
                    <Input
                      value={config.description || ''}
                      onChange={(e) => updateConfig('description')(e.target.value)}
                      placeholder={t('visualDashboard.descriptionPlaceholder')}
                      className="h-9"
                    />
                  </Field>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <Checkbox
                        checked={config.showTrend ?? false}
                        onCheckedChange={(checked) => updateConfig('showTrend')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showTrend')}</span>
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
                allowedTypes: ['device-metric', 'system', 'extension', 'transform', 'ai'],
              },
            },
          ],
        }
}

export function getSparklineSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource } = u
  return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label={t('visualDashboard.colorMode')}
                    value={config.colorMode || 'fixed'}
                    onChange={updateConfig('colorMode')}
                    options={[
                      { value: 'auto', label: t('visualDashboard.auto') },
                      { value: 'primary', label: t('visualDashboard.primaryColor') },
                      { value: 'fixed', label: t('visualDashboard.fixedColor') },
                      { value: 'value', label: t('visualDashboard.basedOnValue') },
                    ]}
                  />

                  {(config.colorMode || 'fixed') === 'fixed' && (
                    <ColorPicker
                      value={config.color || chartColorsHex[0]}
                      onChange={(color) => updateConfig('color')(color)}
                      label={t('visualDashboard.fixedModeColor')}
                      presets="primary"
                    />
                  )}

                  <Field>
                    <Label>{t('visualDashboard.maxValue')}</Label>
                    <Input
                      type="number"
                      value={config.maxValue || 100}
                      onChange={(e) => updateConfig('maxValue')(Number(e.target.value))}
                      min={1}
                      className="h-9"
                    />
                  </Field>

                  <Field>
                    <Label>{t('visualDashboard.lineWidth')}</Label>
                    <Input
                      type="number"
                      value={config.strokeWidth ?? 2}
                      onChange={(e) => updateConfig('strokeWidth')(Number(e.target.value))}
                      min={1}
                      max={5}
                      className="h-9"
                    />
                  </Field>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <Checkbox
                      checked={config.fill ?? true}
                      onCheckedChange={(checked) => updateConfig('fill')(!!checked)}
                    />
                    <span className="text-sm">{t('visualDashboard.fillArea')}</span>
                  </label>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <Checkbox
                      checked={config.curved ?? true}
                      onCheckedChange={(checked) => updateConfig('curved')(!!checked)}
                    />
                    <span className="text-sm">{t('visualDashboard.curved')}</span>
                  </label>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <label className="flex items-center gap-2 cursor-pointer">
                    <Checkbox
                      checked={config.showValue ?? true}
                      onCheckedChange={(checked) => updateConfig('showValue')(!!checked)}
                    />
                    <span className="text-sm">{t('visualDashboard.showCurrentValue')}</span>
                  </label>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <Checkbox
                      checked={config.showThreshold ?? false}
                      onCheckedChange={(checked) => updateConfig('showThreshold')(!!checked)}
                    />
                    <span className="text-sm">{t('visualDashboard.showThreshold')}</span>
                  </label>

                  {config.showThreshold && (
                    <>
                      <Field>
                        <Label>{t('visualDashboard.threshold')}</Label>
                        <Input
                          type="number"
                          value={config.threshold ?? 20}
                          onChange={(e) => updateConfig('threshold')(Number(e.target.value))}
                          className="h-9"
                        />
                      </Field>

                      <ColorPicker
                        value={config.thresholdColor || chartColorsHex[3]}
                        onChange={(color) => updateConfig('thresholdColor')(color)}
                        label={t('visualDashboard.thresholdColor')}
                        presets="semantic"
                      />
                    </>
                  )}
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
                allowedTypes: ['device-metric', 'system', 'extension', 'transform', 'ai'],
              },
            },
          ],
        }
}

export function getProgressBarSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource, updateDataMapping } = u
  return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label={t('visualDashboard.variant')}
                    value={config.variant || 'default'}
                    onChange={updateConfig('variant')}
                    options={[
                      { value: 'default', label: t('visualDashboard.default') },
                      { value: 'icon', label: t('visualDashboard.icon') },
                      { value: 'circular', label: t('visualDashboard.circular') },
                    ]}
                  />

                  {/* Icon variant options */}
                  {config.variant === 'icon' && (
                    <>
                      <IconPicker
                        value={config.icon || ''}
                        onChange={(iconName) => updateConfig('icon')(iconName || undefined)}
                        label={t('visualDashboard.selectIcon')}
                      />

                      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                        <ColorPicker
                          value={config.iconColor || ''}
                          onChange={(color) => updateConfig('iconColor')(color || undefined)}
                          label={t('visualDashboard.iconColor')}
                          presets="primary"
                        />
                        <ColorPicker
                          value={config.backgroundColor || ''}
                          onChange={(color) => updateConfig('backgroundColor')(color || undefined)}
                          label={t('visualDashboard.backgroundColor')}
                          presets="neutral"
                        />
                      </div>
                    </>
                  )}

                  {/* Non-icon variants: custom color */}
                  {config.variant !== 'icon' && (
                    <ColorPicker
                      value={config.color || ''}
                      onChange={(color) => updateConfig('color')(color || undefined)}
                      label={t('visualDashboard.customColor')}
                      presets="primary"
                    />
                  )}

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

                  <label className="flex items-center gap-2 cursor-pointer">
                    <Checkbox
                      id="showCard"
                      checked={config.showCard ?? true}
                      onCheckedChange={(checked) => updateConfig('showCard')(checked === true)}
                    />
                    <label htmlFor="showCard" className="text-sm cursor-pointer">
                      {t('visualDashboard.showCard')}
                    </label>
                  </label>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.warningThreshold')}</Label>
                      <Input
                        type="number"
                        value={config.warningThreshold ?? 70}
                        onChange={(e) => updateConfig('warningThreshold')(Number(e.target.value))}
                        min={0}
                        max={100}
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.dangerThreshold')}</Label>
                      <Input
                        type="number"
                        value={config.dangerThreshold ?? 90}
                        onChange={(e) => updateConfig('dangerThreshold')(Number(e.target.value))}
                        min={0}
                        max={100}
                        className="h-9"
                      />
                    </Field>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {t('visualDashboard.thresholdHint')}
                  </p>

                  <Field>
                    <Label>{t('visualDashboard.maxValue')}</Label>
                    <Input
                      type="number"
                      value={config.max ?? 100}
                      onChange={(e) => updateConfig('max')(Number(e.target.value))}
                      min={1}
                      className="h-9"
                    />
                  </Field>
                </div>
              ),
            },
            {
              type: 'custom' as const,
              render: () => (
                <DataMappingConfig
                  dataMapping={config.dataMapping as SingleValueMappingConfig}
                  onChange={updateDataMapping}
                  mappingType="single"
                />
              ),
            },
          ],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric', 'system', 'extension', 'transform', 'ai'],
              },
            },
          ],
        }
}

export function getLEDIndicatorSchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
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

                  <div className="flex items-center gap-6">
                    <div className="flex items-center gap-2">
                      <Checkbox
                        id="showGlow"
                        checked={config.showGlow ?? true}
                        onCheckedChange={(checked) => updateConfig('showGlow')(checked === true)}
                      />
                      <label htmlFor="showGlow" className="text-sm cursor-pointer">
                        {t('visualDashboard.glowEffect')}
                      </label>
                    </div>

                    <div className="flex items-center gap-2">
                      <Checkbox
                        id="showAnimation"
                        checked={config.showAnimation ?? true}
                        onCheckedChange={(checked) => updateConfig('showAnimation')(checked === true)}
                      />
                      <label htmlFor="showAnimation" className="text-sm cursor-pointer">
                        {t('visualDashboard.animationEffect')}
                      </label>
                    </div>

                    <div className="flex items-center gap-2">
                      <Checkbox
                        id="showCard"
                        checked={config.showCard ?? true}
                        onCheckedChange={(checked) => updateConfig('showCard')(checked === true)}
                      />
                      <label htmlFor="showCard" className="text-sm cursor-pointer">
                        {t('visualDashboard.showCard')}
                      </label>
                    </div>
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
                  {/* Default State - shown when no data source is configured */}
                  <Field>
                    <Label>{t('visualDashboard.defaultState')}</Label>
                    <Select
                      value={config.defaultState || 'unknown'}
                      onValueChange={updateConfig('defaultState')}
                    >
                      <SelectTrigger className="h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="on">{t('visualDashboard.on')}</SelectItem>
                        <SelectItem value="off">{t('visualDashboard.off')}</SelectItem>
                        <SelectItem value="error">{t('visualDashboard.error')}</SelectItem>
                        <SelectItem value="warning">{t('visualDashboard.warning')}</SelectItem>
                        <SelectItem value="unknown">{t('visualDashboard.unknown')}</SelectItem>
                      </SelectContent>
                    </Select>
                    <p className="text-xs text-muted-foreground mt-1">
                      {t('visualDashboard.defaultStateHint')}
                    </p>
                  </Field>

                  {/* State Mapping Rules */}
                  <div className="pt-2 border-t">
                    <LEDStateRulesConfig
                      rules={config.rules as StateRule[] || []}
                      onChange={(newRules) => updateConfig('rules')(newRules)}
                      readonly={!config.dataSource}
                    />
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
                allowedTypes: ['device-metric', 'system', 'extension', 'transform', 'ai'],
              },
            },
          ],
        }
}
