/**
 * Shared markdown styling for release notes inside the upgrade dialogs.
 *
 * Notes are inline context, not a document: headings drop to the body size,
 * margins tighten, and list markers go quiet — so the dialog keeps ONE title
 * level and the changelog reads as flowing text instead of a nested document.
 */
export const releaseNotesProse = [
  'prose prose-sm dark:prose-invert max-w-none text-body',
  '[&>*:first-child]:mt-0',
  'prose-headings:my-1.5 prose-headings:text-body prose-headings:font-semibold',
  'prose-p:my-1.5',
  'prose-ul:my-1.5 prose-ul:pl-4 prose-ol:my-1.5 prose-ol:pl-4',
  'prose-li:my-0.5 prose-li:marker:text-muted-foreground',
  'prose-hr:my-3',
  'prose-a:font-normal',
].join(' ')
