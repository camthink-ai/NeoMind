import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

// Import translation files
import en_common from './locales/en/common.json';
import en_navigation from './locales/en/navigation.json';
import en_devices from './locales/en/devices.json';
import en_alerts from './locales/en/alerts.json';
import en_automation from './locales/en/automation.json';
import en_commands from './locales/en/commands.json';
import en_plugins from './locales/en/plugins.json';
import en_extensions from './locales/en/extensions.json';
import en_settings from './locales/en/settings.json';
import en_auth from './locales/en/auth.json';
import en_validation from './locales/en/validation.json';
import en_messages from './locales/en/messages.json';
import en_dashboard from './locales/en/dashboard.json';
import en_agents from './locales/en/agents.json';
import en_dashboard_components from './locales/en/dashboard-components.json';
import en_chat from './locales/en/chat.json';
import en_setup from './locales/en/setup.json';

import zh_common from './locales/zh/common.json';
import zh_navigation from './locales/zh/navigation.json';
import zh_devices from './locales/zh/devices.json';
import zh_alerts from './locales/zh/alerts.json';
import zh_automation from './locales/zh/automation.json';
import zh_commands from './locales/zh/commands.json';
import zh_plugins from './locales/zh/plugins.json';
import zh_extensions from './locales/zh/extensions.json';
import zh_settings from './locales/zh/settings.json';
import zh_auth from './locales/zh/auth.json';
import zh_validation from './locales/zh/validation.json';
import zh_messages from './locales/zh/messages.json';
import zh_dashboard from './locales/zh/dashboard.json';
import zh_agents from './locales/zh/agents.json';
import zh_dashboard_components from './locales/zh/dashboard-components.json';
import zh_chat from './locales/zh/chat.json';
import zh_setup from './locales/zh/setup.json';

const resources = {
  en: {
    common: en_common,
    navigation: en_navigation,
    devices: en_devices,
    alerts: en_alerts,
    automation: en_automation,
    commands: en_commands,
    plugins: en_plugins,
    extensions: en_extensions,
    settings: en_settings,
    auth: en_auth,
    validation: en_validation,
    messages: en_messages,
    dashboard: en_dashboard,
    agents: en_agents,
    dashboardComponents: en_dashboard_components,
    chat: en_chat,
    setup: en_setup,
  },
  zh: {
    common: zh_common,
    navigation: zh_navigation,
    devices: zh_devices,
    alerts: zh_alerts,
    automation: zh_automation,
    commands: zh_commands,
    plugins: zh_plugins,
    extensions: zh_extensions,
    settings: zh_settings,
    auth: zh_auth,
    validation: zh_validation,
    messages: zh_messages,
    dashboard: zh_dashboard,
    agents: zh_agents,
    dashboardComponents: zh_dashboard_components,
    chat: zh_chat,
    setup: zh_setup,
  },
};

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    lng: 'en', // Default to English
    fallbackLng: 'en',
    defaultNS: 'common',
    ns: ['common', 'navigation', 'devices', 'alerts', 'automation',
         'commands', 'plugins', 'extensions', 'settings', 'auth',
         'validation', 'messages', 'dashboard', 'agents', 'dashboardComponents', 'chat', 'setup'],
    interpolation: {
      escapeValue: false,
    },
    detection: {
      order: ['localStorage', 'navigator'],
      caches: ['localStorage'],
    },
  });

export default i18n;
