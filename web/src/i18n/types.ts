declare module 'react-i18next' {
  interface CustomTypeOptions {
    resources: {
      common: typeof import('./locales/en/common.json')
      navigation: typeof import('./locales/en/navigation.json')
      devices: typeof import('./locales/en/devices.json')
      alerts: typeof import('./locales/en/alerts.json')
      automation: typeof import('./locales/en/automation.json')
      commands: typeof import('./locales/en/commands.json')
      decisions: typeof import('./locales/en/decisions.json')
      plugins: typeof import('./locales/en/plugins.json')
      settings: typeof import('./locales/en/settings.json')
      auth: typeof import('./locales/en/auth.json')
      validation: typeof import('./locales/en/validation.json')
      messages: typeof import('./locales/en/messages.json')
      dashboard: typeof import('./locales/en/dashboard.json')
    }
    defaultNS: 'common'
    returnNull: false
  }
}
