/// <reference types="react-i18next" />

declare module 'react-i18next' {
  import * as i18nReact from 'react-i18next'
  export const useTranslation: typeof i18nReact.useTranslation
  export const initReactI18next: typeof i18nReact.initReactI18next
  export const I18nextProvider: typeof i18nReact.I18nextProvider
  export const Trans: typeof i18nReact.Trans
}
