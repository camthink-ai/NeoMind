/**
 * NeoMind Extension Frontend SDK
 *
 * This module provides types, utilities, and templates for building
 * frontend components that integrate with NeoMind extensions.
 *
 * @example
 * ```tsx
 * import { ExtensionCard, executeExtensionCommand } from '@neomind/extension-sdk/frontend'
 *
 * const MyCard = () => {
 *   const [data, setData] = useState(null)
 *
 *   useEffect(() => {
 *     executeExtensionCommand('my-extension', 'get_status')
 *       .then(result => setData(result.data))
 *   }, [])
 *
 *   return <ExtensionCard title="My Extension">{JSON.stringify(data)}</ExtensionCard>
 * }
 * ```
 */

// Types
export * from './types'

// Component Template
export { ExtensionCard, componentMetadata } from './component'
export type { ExtensionCardProps } from './component'
