export {
  FRAGMENT_VECTORS,
  exactSizedJson,
  type FragmentVector,
  type FragmentExpect,
} from './fragment'
export {
  ENVELOPE_VECTORS,
  envelopeWireBytes,
  type EnvelopeVector,
} from './envelope'
export {
  ADVERTISING_VECTORS,
  NAME_VECTORS,
  META_VECTORS,
  type AdvertisingVector,
  type NameVector,
  type MetaVector,
} from './advertising'

/** Pinned source of golden vectors. */
export const NEOLINK_VECTORS_REF = {
  repo: 'https://github.com/Oreofilling/neolink',
  commit: 'a22c7f28deef27119a9adb03e07f8e7e2bd07a1f',
  packages: ['packages/test_vectors', 'packages/ct_prov_core', 'packages/ble_adapter'],
} as const
