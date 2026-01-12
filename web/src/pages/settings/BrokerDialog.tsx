import { useState, useEffect } from "react"
import type { ExternalBroker } from "@/types"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Checkbox } from "@/components/ui/checkbox"
import { toast } from "@/components/ui/use-toast"

interface BrokerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  editingBroker: ExternalBroker | null
  isSaving: boolean
  onSave: (data: {
    name: string
    broker: string
    port: number
    tls: boolean
    username?: string
    password?: string
    ca_cert?: string
    client_cert?: string
    client_key?: string
    enabled: boolean
  }) => Promise<void>
}

export function BrokerDialog({
  open,
  onOpenChange,
  editingBroker,
  isSaving,
  onSave,
}: BrokerDialogProps) {
  const [name, setName] = useState("")
  const [address, setAddress] = useState("192.168.1.100")
  const [port, setPort] = useState(1883)
  const [tls, setTls] = useState(false)
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [enabled, setEnabled] = useState(true)
  const [caCert, setCaCert] = useState("")
  const [clientCert, setClientCert] = useState("")
  const [clientKey, setClientKey] = useState("")

  // Reset form when editing broker changes
  useEffect(() => {
    if (editingBroker) {
      setName(editingBroker.name)
      setAddress(editingBroker.broker)
      setPort(editingBroker.port)
      setTls(editingBroker.tls || false)
      setUsername(editingBroker.username || "")
      setPassword("") // Don't show existing password
      setEnabled(editingBroker.enabled)
      setCaCert(editingBroker.ca_cert || "")
      setClientCert(editingBroker.client_cert || "")
      setClientKey(editingBroker.client_key || "")
    } else {
      setName("")
      setAddress("192.168.1.100")
      setPort(1883)
      setTls(false)
      setUsername("")
      setPassword("")
      setEnabled(true)
      setCaCert("")
      setClientCert("")
      setClientKey("")
    }
  }, [editingBroker])

  const handleSave = async () => {
    // Validate
    if (!name.trim()) {
      toast({ title: "请输入名称", variant: "destructive" })
      return
    }
    if (!address.trim()) {
      toast({ title: "请输入地址", variant: "destructive" })
      return
    }
    if (port < 1 || port > 65535) {
      toast({ title: "端口无效", variant: "destructive" })
      return
    }

    await onSave({
      name,
      broker: address,
      port,
      tls,
      username: username || undefined,
      password: password || undefined,
      ca_cert: caCert || undefined,
      client_cert: clientCert || undefined,
      client_key: clientKey || undefined,
      enabled,
    })
  }

  const handleTlsChange = (newTls: boolean) => {
    setTls(newTls)
    // Auto-switch port if using default
    if ((newTls && port === 1883) || (!newTls && port === 8883)) {
      setPort(newTls ? 8883 : 1883)
    }
    // When enabling TLS, also enable the broker
    if (newTls && !enabled) {
      setEnabled(true)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{editingBroker ? "编辑 Broker" : "添加外部 Broker"}</DialogTitle>
          <DialogDescription>
            配置外部 MQTT Broker 连接信息，系统将订阅该 Broker 的设备数据。
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4 py-4">
          <div className="space-y-2">
            <Label htmlFor="broker-name">名称 *</Label>
            <Input
              id="broker-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如: 生产环境 Broker"
            />
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="broker-address">地址 *</Label>
              <Input
                id="broker-address"
                value={address}
                onChange={(e) => setAddress(e.target.value)}
                placeholder="192.168.1.100"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="broker-port">端口 *</Label>
              <Input
                id="broker-port"
                type="number"
                value={port}
                onChange={(e) => setPort(Number(e.target.value))}
                min={1}
                max={65535}
              />
            </div>
          </div>

          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor="broker-tls">使用 TLS/MQTTS</Label>
              <p className="text-xs text-muted-foreground">
                启用后使用 mqtts:// 协议，默认端口为 8883
              </p>
            </div>
            <Checkbox
              id="broker-tls"
              checked={tls}
              onCheckedChange={(checked) => handleTlsChange(checked === true)}
            />
          </div>

          {/* TLS Certificate Configuration */}
          {tls && (
            <div className="space-y-4 p-4 border rounded-lg bg-muted/30">
              <div className="flex items-center gap-2 text-sm font-medium">
                <span className="text-blue-600">🔒</span>
                TLS 证书配置
              </div>

              <div className="space-y-2">
                <Label htmlFor="ca-cert" className="text-xs">CA 证书 (PEM格式) - 可选</Label>
                <Textarea
                  id="ca-cert"
                  value={caCert}
                  onChange={(e) => setCaCert(e.target.value)}
                  placeholder="-----BEGIN CERTIFICATE-----&#10;用于验证服务器证书"
                  className="font-mono text-xs h-20 resize-none"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="client-cert" className="text-xs">客户端证书 (PEM格式) - 可选</Label>
                <Textarea
                  id="client-cert"
                  value={clientCert}
                  onChange={(e) => setClientCert(e.target.value)}
                  placeholder="-----BEGIN CERTIFICATE-----&#10;用于 mTLS 双向认证"
                  className="font-mono text-xs h-20 resize-none"
                />
              </div>

              <div className="space-y-2">
                <Label htmlFor="client-key" className="text-xs">客户端私钥 (PEM格式) - 可选</Label>
                <Textarea
                  id="client-key"
                  value={clientKey}
                  onChange={(e) => setClientKey(e.target.value)}
                  placeholder="-----BEGIN PRIVATE KEY-----&#10;配合客户端证书使用"
                  className="font-mono text-xs h-20 resize-none"
                />
              </div>
            </div>
          )}

          <div className="space-y-2">
            <Label htmlFor="broker-username">用户名 (可选)</Label>
            <Input
              id="broker-username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="留空则不使用认证"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="broker-password">密码 (可选)</Label>
            <Input
              id="broker-password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={editingBroker ? "留空保持原密码不变" : "留空则不使用认证"}
            />
          </div>

          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor="broker-enabled">启用此 Broker</Label>
              <p className="text-xs text-muted-foreground">
                {tls ? "TLS 已启用，Broker 将自动连接" : "禁用后将停止接收此 Broker 的数据"}
              </p>
            </div>
            <Checkbox
              id="broker-enabled"
              checked={enabled}
              onCheckedChange={(checked) => setEnabled(checked === true)}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={handleSave} disabled={isSaving}>
            {isSaving ? "保存中..." : "保存"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
