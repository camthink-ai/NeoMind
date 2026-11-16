// Web Bluetooth API type declarations for CamThink CT-Prov BLE provisioning
// Minimal types required by ct-prov ble-channel (Web Bluetooth path)

interface BluetoothDevice extends EventTarget {
  readonly gatt?: BluetoothRemoteGATTServer
  readonly id: string
  readonly name?: string
  addEventListener(type: 'gattserverdisconnected', listener: () => void): void
  removeEventListener(type: 'gattserverdisconnected', listener: () => void): void
}

interface BluetoothRemoteGATTServer {
  readonly connected: boolean
  connect(): Promise<BluetoothRemoteGATTServer>
  disconnect(): void
  getPrimaryService(service: BluetoothServiceUUID): Promise<BluetoothRemoteGATTService>
}

interface BluetoothRemoteGATTService {
  getCharacteristic(characteristic: BluetoothCharacteristicUUID): Promise<BluetoothRemoteGATTCharacteristic>
}

interface BluetoothRemoteGATTCharacteristic extends EventTarget {
  readonly value: DataView | null
  readonly uuid: string
  readValue(): Promise<DataView>
  writeValue(value: BufferSource | Uint8Array): Promise<void>
  writeValueWithResponse(value: BufferSource | Uint8Array): Promise<void>
  startNotifications(): Promise<BluetoothRemoteGATTCharacteristic>
  stopNotifications(): Promise<BluetoothRemoteGATTCharacteristic>
  addEventListener(
    type: 'characteristicvaluechanged',
    listener: (event: Event) => void,
  ): void
  removeEventListener(
    type: 'characteristicvaluechanged',
    listener: (event: Event) => void,
  ): void
}

type BluetoothServiceUUID = string | number
type BluetoothCharacteristicUUID = string | number

interface BluetoothRequestDeviceFilter {
  services?: BluetoothServiceUUID[]
  name?: string
  namePrefix?: string
}

interface BluetoothRequestDeviceOptions {
  filters: BluetoothRequestDeviceFilter[]
  optionalServices?: BluetoothServiceUUID[]
}

interface Bluetooth {
  requestDevice(options?: BluetoothRequestDeviceOptions): Promise<BluetoothDevice>
}

interface Navigator {
  readonly bluetooth?: Bluetooth
}
