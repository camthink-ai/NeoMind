import React from 'react'
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
import { DualModeSourceField } from '@/components/dashboard/config'
import type { ComponentConfigSchema } from '@/components/dashboard/config/ComponentConfigBuilder'
import { SelectField } from '../../ConfigFieldComponents'
import type { SchemaContext, Updaters } from '../types'

export function getImageDisplaySchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource } = u
  return {
          dataSourceSections: [
            {
              type: 'custom' as const,
              render: () => (
                <DualModeSourceField
                  inputType="image"
                  value={config.src || ''}
                  onValueChange={updateConfig('src')}
                  dataSource={config.dataSource}
                  onDataSourceChange={updateDataSource}
                  allowedTypes={['device-metric', 'system', 'extension', 'transform', 'ai']}
                  label={t('visualDashboard.imageSource')}
                  placeholder={t('visualDashboard.urlPlaceholder')}
                />
              ),
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label={t('visualDashboard.fitMode')}
                    value={config.fit || 'contain'}
                    onChange={updateConfig('fit')}
                    options={[
                      { value: 'contain', label: t('visualDashboard.fitContain') },
                      { value: 'cover', label: t('visualDashboard.fitCover') },
                      { value: 'fill', label: t('visualDashboard.fitFill') },
                      { value: 'none', label: t('visualDashboard.fitNone') },
                      { value: 'scale-down', label: t('visualDashboard.fitScaleDown') },
                    ]}
                  />
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2">
                      <Checkbox
                        checked={config.rounded ?? true}
                        onCheckedChange={(checked) => updateConfig('rounded')(!!checked)}
                      />
                      <span className="text-xs">{t('visualDashboard.rounded')}</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <Checkbox
                        checked={config.zoomable ?? true}
                        onCheckedChange={(checked) => updateConfig('zoomable')(!!checked)}
                      />
                      <span className="text-xs">{t('visualDashboard.zoomable')}</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <Checkbox
                        checked={config.showShadow ?? false}
                        onCheckedChange={(checked) => updateConfig('showShadow')(!!checked)}
                      />
                      <span className="text-xs">{t('visualDashboard.shadow')}</span>
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
                  <Field>
                    <Label>{t('imageDisplay.altText')}</Label>
                    <Input
                      value={config.alt || ''}
                      onChange={(e) => updateConfig('alt')(e.target.value)}
                      placeholder={t('placeholders.imageAltText')}
                      className="h-9"
                    />
                    <p className="text-xs text-muted-foreground mt-1">
                      {t('imageDisplay.altHint', 'Alternative text for screen readers and when image fails to load')}
                    </p>
                  </Field>

                  <Field>
                    <Label>{t('visualDashboard.title')}</Label>
                    <Input
                      value={config.title || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('placeholders.imageTitle')}
                      className="h-9"
                    />
                  </Field>

                  <Field>
                    <Label>{t('imageDisplay.caption', 'Caption')}</Label>
                    <Input
                      value={config.caption || ''}
                      onChange={(e) => updateConfig('caption')(e.target.value)}
                      placeholder={t('placeholders.imageCaption')}
                      className="h-9"
                    />
                  </Field>

                  <SelectField
                    label={t('placeholders.loadingState')}
                    value={config.loadingState || 'lazy'}
                    onChange={updateConfig('loadingState')}
                    options={[
                      { value: 'eager', label: t('imageDisplay.loadImmediately', 'Load Immediately') },
                      { value: 'lazy', label: t('imageDisplay.lazyLoad', 'Lazy Load') },
                    ]}
                  />
                </div>
              ),
            },
          ],
        }
}

export function getImageHistorySchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource } = u
  return {
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
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label={t('visualDashboard.fitMode')}
                    value={config.fit || 'contain'}
                    onChange={updateConfig('fit')}
                    options={[
                      { value: 'contain', label: t('visualDashboard.fitContain') },
                      { value: 'cover', label: t('visualDashboard.fitCover') },
                      { value: 'fill', label: t('visualDashboard.fitFill') },
                      { value: 'none', label: t('visualDashboard.fitNone') },
                      { value: 'scale-down', label: t('visualDashboard.fitScaleDown') },
                    ]}
                  />
                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.maxImages')}</Label>
                      <Input
                        type="number"
                        value={config.limit !== undefined && config.limit !== null && config.limit !== '' ? config.limit : ''}
                        onChange={(e) => {
                          const raw = e.target.value
                          if (raw === '') {
                            updateConfig('limit')(undefined)
                            return
                          }
                          const v = Number(raw)
                          if (Number.isFinite(v)) updateConfig('limit')(v)
                        }}
                        onBlur={(e) => {
                          const v = Number(e.target.value)
                          if (e.target.value !== '' && (!Number.isFinite(v) || v < 1 || v > 200)) {
                            updateConfig('limit')(50)
                          }
                        }}
                        min={1}
                        max={200}
                        placeholder="50"
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.timeRangeHours')}</Label>
                      <Input
                        type="number"
                        value={config.timeRange ?? 1}
                        onChange={(e) => updateConfig('timeRange')(Number(e.target.value))}
                        min={1}
                        max={168}
                        step={1}
                        className="h-9"
                      />
                    </Field>
                  </div>
                  <div className="flex flex-wrap items-center gap-3">
                    <label className="flex items-center gap-2">
                      <Checkbox
                        checked={config.rounded ?? true}
                        onCheckedChange={(checked) => updateConfig('rounded')(!!checked)}
                      />
                      <span className="text-xs">{t('visualDashboard.rounded')}</span>
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
                  {/* Only fields the component actually consumes. The old
                      alt / showNavigation / showDots / autoPlay(+interval)
                      controls wrote keys ImageHistory never read — the
                      component is a slider viewer, not a carousel. */}
                  <Field>
                    <Label>{t('visualDashboard.title')}</Label>
                    <Input
                      value={config.title || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('placeholders.galleryTitle')}
                      className="h-9"
                    />
                  </Field>
                </div>
              ),
            },
          ],
        }
}

export function getWebDisplaySchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource } = u
  return {
          dataSourceSections: [
            {
              type: 'custom' as const,
              render: () => (
                <DualModeSourceField
                  inputType="url"
                  value={config.src || ''}
                  onValueChange={updateConfig('src')}
                  dataSource={config.dataSource}
                  onDataSourceChange={updateDataSource}
                  allowedTypes={['device-metric', 'system', 'extension', 'transform', 'ai']}
                  label={t('webDisplay.websiteUrl', 'Website URL')}
                  placeholder={t('placeholders.urlExample')}
                />
              ),
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2">
                      <Checkbox
                        checked={config.sandbox ?? true}
                        onCheckedChange={(checked) => updateConfig('sandbox')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.sandboxIsolation')}</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <Checkbox
                        checked={config.showHeader ?? true}
                        onCheckedChange={(checked) => updateConfig('showHeader')(!!checked)}
                      />
                      <span className="text-sm">{t('visualDashboard.showHeader')}</span>
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
                  <Field>
                    <Label>{t('visualDashboard.title')}</Label>
                    <Input
                      value={config.title || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('placeholders.websiteTitle')}
                      className="h-9"
                    />
                  </Field>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <Checkbox
                      /* Reads the legacy misspelled key (allowFullScreen) for
                         configs saved before the rename — the component prop
                         is allowFullscreen, the old key never reached it. */
                      checked={config.allowFullscreen ?? config.allowFullScreen ?? true}
                      onCheckedChange={(checked) => updateConfig('allowFullscreen')(!!checked)}
                    />
                    <span className="text-sm">{t('webDisplay.allowFullscreen', 'Allow Fullscreen')}</span>
                  </label>
                </div>
              ),
            },
          ],
        }
}

export function getMarkdownDisplaySchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource } = u
  return {
          dataSourceSections: [
            {
              type: 'custom' as const,
              render: () => (
                <DualModeSourceField
                  inputType="text"
                  value={config.content || ''}
                  onValueChange={updateConfig('content')}
                  dataSource={config.dataSource}
                  onDataSourceChange={updateDataSource}
                  allowedTypes={['device-metric', 'system', 'extension', 'transform', 'ai']}
                  label={t('visualDashboard.markdownContent')}
                  placeholder={t('visualDashboard.markdownPlaceholder')}
                  rows={6}
                />
              ),
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label={t('visualDashboard.style')}
                    value={config.variant || 'default'}
                    onChange={updateConfig('variant')}
                    options={[
                      { value: 'default', label: t('visualDashboard.default') },
                      { value: 'compact', label: t('visualDashboard.compact') },
                      { value: 'minimal', label: t('visualDashboard.minimal') },
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
                  <Field>
                    <Label>{t('visualDashboard.title')}</Label>
                    <Input
                      value={config.title || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('placeholders.contentTitle')}
                      className="h-9"
                    />
                  </Field>
                </div>
              ),
            },
          ],
        }
}

export function getVideoDisplaySchema(config: any, ctx: SchemaContext, u: Updaters): ComponentConfigSchema {
  const { t } = ctx
  const { updateConfig, updateDataSource } = u
  return {
          dataSourceSections: [
            {
              type: 'custom' as const,
              render: () => (
                <DualModeSourceField
                  inputType="url"
                  value={config.src || ''}
                  onValueChange={updateConfig('src')}
                  dataSource={config.dataSource}
                  onDataSourceChange={updateDataSource}
                  allowedTypes={['device', 'device-info', 'device-metric']}
                  label={t('visualDashboard.videoSource')}
                  placeholder={t('visualDashboard.videoUrlPlaceholder')}
                />
              ),
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label={t('visualDashboard.videoType')}
                    value={config.type || 'file'}
                    onChange={updateConfig('type')}
                    options={[
                      { value: 'file', label: t('visualDashboard.videoFile') },
                      { value: 'hls', label: t('videoDisplay.hlsLabel') },
                      { value: 'device-camera', label: t('visualDashboard.deviceCamera') },
                    ]}
                  />

                  {/* Type-specific hints */}
                  {config.type === 'hls' && (
                    <div className="p-2 bg-success-light border border-success-light rounded-md">
                      <p className="text-xs text-success">
                        <strong>{t('videoDisplay.hlsUrlFormat')}:</strong> http://server/path/index.m3u8
                      </p>
                    </div>
                  )}

                  {config.type === 'device-camera' && (
                    <div className="p-2 bg-info-light border border-info rounded-md">
                      <p className="text-xs text-info">
                        <strong>{t('visualDashboard.deviceCamera')}:</strong> {t('videoDisplay.deviceCameraHint')}
                      </p>
                    </div>
                  )}

                  <SelectField
                    label={t('visualDashboard.fitMethod')}
                    value={config.fit || 'contain'}
                    onChange={updateConfig('fit')}
                    options={[
                      { value: 'contain', label: t('visualDashboard.fitContainFull') },
                      { value: 'cover', label: t('visualDashboard.fitCoverFill') },
                      { value: 'fill', label: t('visualDashboard.fitStretch') },
                    ]}
                  />

                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.autoPlay')}</Label>
                      <Select
                        value={String(config.autoplay ?? false)}
                        onValueChange={(value) => updateConfig('autoplay')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="false">{t('visualDashboard.off')}</SelectItem>
                          <SelectItem value="true">{t('visualDashboard.on')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.muted')}</Label>
                      <Select
                        value={String(config.muted ?? true)}
                        onValueChange={(value) => updateConfig('muted')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t('visualDashboard.muted')}</SelectItem>
                          <SelectItem value="false">{t('visualDashboard.unmuted')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  </div>

                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
                    <Field>
                      <Label>{t('visualDashboard.showControls')}</Label>
                      <Select
                        value={String(config.controls ?? true)}
                        onValueChange={(value) => updateConfig('controls')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t('visualDashboard.showCard')}</SelectItem>
                          <SelectItem value="false">{t('visualDashboard.hide')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <Label>{t('visualDashboard.loop')}</Label>
                      <Select
                        value={String(config.loop ?? false)}
                        onValueChange={(value) => updateConfig('loop')(value === 'true')}
                      >
                        <SelectTrigger className="w-full h-9">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="false">{t('visualDashboard.off')}</SelectItem>
                          <SelectItem value="true">{t('visualDashboard.on')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  </div>

                  <Field>
                    <Label>{t('visualDashboard.fullscreenButton')}</Label>
                    <Select
                      value={String(config.showFullscreen ?? true)}
                      onValueChange={(value) => updateConfig('showFullscreen')(value === 'true')}
                    >
                      <SelectTrigger className="w-full h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="true">{t('visualDashboard.showCard')}</SelectItem>
                        <SelectItem value="false">{t('visualDashboard.hide')}</SelectItem>
                      </SelectContent>
                    </Select>
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
                  <Field>
                    <Label>{t('videoDisplay.posterImageUrl', 'Poster Image URL')}</Label>
                    <Input
                      value={config.poster || ''}
                      onChange={(e) => updateConfig('poster')(e.target.value)}
                      placeholder={t('videoDisplay.posterPlaceholder', 'https://example.com/poster.jpg')}
                      className="h-9"
                    />
                    <p className="text-xs text-muted-foreground mt-1">
                      {t('videoDisplay.posterHint', 'Image shown before video plays')}
                    </p>
                  </Field>

                  <Field>
                    <Label>{t('visualDashboard.title')}</Label>
                    <Input
                      value={config.title || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('placeholders.videoTitle')}
                      className="h-9"
                    />
                  </Field>
                </div>
              ),
            },
          ],
        }
}
