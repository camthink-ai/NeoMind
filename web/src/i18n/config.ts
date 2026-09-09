import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

// Import translation files
import en_common from './locales/en/common.json';
import en_devices from './locales/en/devices.json';
import en_alerts from './locales/en/alerts.json';
import en_automation from './locales/en/automation.json';
import en_plugins from './locales/en/plugins.json';
import en_extensions from './locales/en/extensions.json';
import en_settings from './locales/en/settings.json';
import en_auth from './locales/en/auth.json';
import en_validation from './locales/en/validation.json';
import en_dashboard from './locales/en/dashboard.json';
import en_agents from './locales/en/agents.json';
import en_dashboard_components from './locales/en/dashboard-components.json';
import en_chat from './locales/en/chat.json';
import en_setup from './locales/en/setup.json';
import en_data from './locales/en/data.json';
import en_instances from './locales/en/instances.json';
import en_ui from './locales/en/ui.json';

import zh_common from './locales/zh/common.json';
import zh_devices from './locales/zh/devices.json';
import zh_alerts from './locales/zh/alerts.json';
import zh_automation from './locales/zh/automation.json';
import zh_plugins from './locales/zh/plugins.json';
import zh_extensions from './locales/zh/extensions.json';
import zh_settings from './locales/zh/settings.json';
import zh_auth from './locales/zh/auth.json';
import zh_validation from './locales/zh/validation.json';
import zh_dashboard from './locales/zh/dashboard.json';
import zh_agents from './locales/zh/agents.json';
import zh_dashboard_components from './locales/zh/dashboard-components.json';
import zh_chat from './locales/zh/chat.json';
import zh_setup from './locales/zh/setup.json';
import zh_data from './locales/zh/data.json';
import zh_instances from './locales/zh/instances.json';
import zh_ui from './locales/zh/ui.json';

const resources = {
  en: {
    common: en_common,
    devices: en_devices,
    alerts: en_alerts,
    automation: en_automation,
    plugins: en_plugins,
    extensions: en_extensions,
    settings: en_settings,
    auth: en_auth,
    validation: en_validation,
    dashboard: en_dashboard,
    agents: en_agents,
    dashboardComponents: en_dashboard_components,
    chat: en_chat,
    setup: en_setup,
    data: en_data,
    instances: en_instances,
    ui: en_ui,
  },
  zh: {
    common: zh_common,
    devices: zh_devices,
    alerts: zh_alerts,
    automation: zh_automation,
    plugins: zh_plugins,
    extensions: zh_extensions,
    settings: zh_settings,
    auth: zh_auth,
    validation: zh_validation,
    dashboard: zh_dashboard,
    agents: zh_agents,
    dashboardComponents: zh_dashboard_components,
    chat: zh_chat,
    setup: zh_setup,
    data: zh_data,
    instances: zh_instances,
    ui: zh_ui,
  },
};

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: 'en',
    defaultNS: 'common',
    ns: ['common', 'devices', 'alerts', 'automation',
         'plugins', 'extensions', 'settings', 'auth',
         'validation', 'dashboard', 'agents', 'dashboardComponents', 'chat', 'setup', 'data', 'instances', 'ui'],
    interpolation: {
      escapeValue: false,
    },
    detection: {
      order: ['localStorage', 'navigator'],
      caches: ['localStorage'],
    },
  });

// Keep <html lang> in sync with the active language — index.html ships a
// static zh-CN and it was never updated, so screen readers and translation
// tools saw Chinese on an English UI (and vice versa after any switch).
const applyHtmlLang = (lng: string) => {
  document.documentElement.lang = lng.startsWith('zh') ? 'zh-CN' : lng || 'en';
};
applyHtmlLang(i18n.language);
i18n.on('languageChanged', applyHtmlLang);

export default i18n;
