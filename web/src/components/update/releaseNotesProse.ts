/**
 * Shared markdown styling for release notes inside the upgrade dialogs.
 *
 * Notes are inline context, not a document: headings drop to the body size,
 * margins tighten, and list markers go quiet — so the dialog keeps ONE title
 * level and the changelog reads as flowing text instead of a nested document.
 */
export const releaseNotesProse = [
  'prose prose-sm dark:prose-invert max-w-none text-sm leading-relaxed break-words',
  '[&>*:first-child]:mt-0 [&>*:last-child]:mb-0',
  'prose-headings:mt-4 prose-headings:mb-1.5 prose-headings:text-sm prose-headings:font-semibold prose-headings:text-foreground',
  'prose-p:my-1.5 prose-p:text-muted-foreground',
  'prose-ul:my-1.5 prose-ul:pl-4 prose-ol:my-1.5 prose-ol:pl-4',
  'prose-li:my-0.5 prose-li:text-muted-foreground prose-li:marker:text-muted-foreground',
  'prose-strong:text-foreground prose-strong:font-medium',
  'prose-code:text-xs prose-code:bg-muted-30 prose-code:px-1 prose-code:py-0.5 prose-code:rounded prose-code:before:content-none prose-code:after:content-none',
  'prose-pre:text-xs prose-pre:bg-muted-20 prose-pre:border prose-pre:border-glass-border prose-pre:rounded-lg prose-pre:overflow-x-auto',
  'prose-hr:my-3 prose-hr:border-glass-border',
  'prose-a:font-normal',
].join(' ')
