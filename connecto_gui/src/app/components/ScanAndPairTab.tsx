import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/app/components/ui/card';
import { Button } from '@/app/components/ui/button';
import { Input } from '@/app/components/ui/input';
import { Badge } from '@/app/components/ui/badge';
import { Wifi, Loader2, CheckCircle2, Monitor, Server, Copy, Link2 } from 'lucide-react';
import { toast } from 'sonner';

interface DeviceInfo {
  name: string;
  hostname: string;
  addresses: string[];
  port: number;
  index: number;
}

interface PairingResult {
  success: boolean;
  server_name: string;
  ssh_user: string;
  ssh_command: string;
  private_key_path: string;
  public_key_path: string;
  error?: string;
}

interface PairedHost {
  host: string;
  hostname: string;
  user: string;
  identity_file: string;
}

export function ScanAndPairTab() {
  const [isScanning, setIsScanning] = useState(false);
  const [manualIp, setManualIp] = useState('');
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [pairingIndex, setPairingIndex] = useState<number | null>(null);
  const [pairedIndices, setPairedIndices] = useState<Set<number>>(new Set());
  const [pairingResult, setPairingResult] = useState<PairingResult | null>(null);
  const [pairedHosts, setPairedHosts] = useState<PairedHost[]>([]);

  // Load paired hosts on mount
  useEffect(() => {
    loadPairedHosts();
  }, []);

  const loadPairedHosts = async () => {
    try {
      const hosts = await invoke<PairedHost[]>('list_paired_hosts');
      setPairedHosts(hosts);
    } catch (error) {
      console.error('Failed to load paired hosts:', error);
    }
  };

  const handleScan = async () => {
    setIsScanning(true);
    toast.info('Scanning network for Connecto devices...');

    try {
      const result = await invoke<DeviceInfo[]>('scan_devices', { timeoutSecs: 5 });
      setDevices(result);
      if (result.length === 0) {
        toast.warning('No devices found on your network');
      } else {
        toast.success(`Found ${result.length} device(s) on your network`);
      }
    } catch (error) {
      toast.error(`Scan failed: ${error}`);
    } finally {
      setIsScanning(false);
    }
  };

  const handlePair = async (device: DeviceInfo) => {
    setPairingIndex(device.index);
    toast.loading(`Pairing with ${extractName(device.name)}...`, { id: 'pairing' });

    try {
      const result = await invoke<PairingResult>('pair_with_device', {
        deviceIndex: device.index,
        useRsa: false,
        customComment: null
      });

      if (result.success) {
        setPairedIndices(prev => new Set(prev).add(device.index));
        setPairingResult(result);
        loadPairedHosts(); // Refresh paired hosts list
        toast.success(`Successfully paired with ${result.server_name}!`, { id: 'pairing' });
      } else {
        toast.error(`Pairing failed: ${result.error}`, { id: 'pairing' });
      }
    } catch (error) {
      toast.error(`Pairing failed: ${error}`, { id: 'pairing' });
    } finally {
      setPairingIndex(null);
    }
  };

  const handleManualConnect = async () => {
    if (!manualIp) {
      toast.error('Please enter an IP address');
      return;
    }

    let address = manualIp;
    if (!address.includes(':')) {
      address = `${address}:8099`;
    }

    toast.loading('Connecting...', { id: 'manual' });

    try {
      const result = await invoke<PairingResult>('pair_with_address', {
        address,
        useRsa: false,
        customComment: null
      });

      if (result.success) {
        setPairingResult(result);
        loadPairedHosts(); // Refresh paired hosts list
        toast.success(`Successfully paired!`, { id: 'manual' });
        setManualIp('');
      } else {
        toast.error(`Connection failed: ${result.error}`, { id: 'manual' });
      }
    } catch (error) {
      toast.error(`Connection failed: ${error}`, { id: 'manual' });
    }
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    toast.success('Copied to clipboard!');
  };

  const extractName = (fullName: string) => {
    return fullName.split('._connecto')[0].replace(/_/g, ' ');
  };

  return (
    <div className="space-y-6">
      {/* Network Discovery */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Network Discovery</CardTitle>
              <CardDescription>Find devices running Connecto on your local network</CardDescription>
            </div>
            <Button onClick={handleScan} disabled={isScanning}>
              {isScanning ? (
                <>
                  <Loader2 className="mr-2 size-4 animate-spin" />
                  Scanning...
                </>
              ) : (
                <>
                  <Wifi className="mr-2 size-4" />
                  Scan Network
                </>
              )}
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          <div className="space-y-3">
            {devices.length === 0 ? (
              <p className="text-center text-gray-500 py-8">
                No devices found. Click "Scan Network" to search.
              </p>
            ) : (
              devices.map((device) => (
                <div
                  key={device.index}
                  className="flex items-center justify-between p-4 border rounded-lg hover:bg-gray-50 transition-colors"
                >
                  <div className="flex items-center gap-4">
                    <div className="p-2 bg-blue-100 rounded-lg">
                      <Monitor className="size-5" />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <p className="font-medium">{extractName(device.name)}</p>
                        {pairedIndices.has(device.index) && (
                          <Badge variant="default" className="bg-green-600">
                            <CheckCircle2 className="mr-1 size-3" />
                            Paired
                          </Badge>
                        )}
                      </div>
                      <p className="text-sm text-gray-500">
                        {device.addresses[0] || 'Unknown'}:{device.port}
                      </p>
                    </div>
                  </div>
                  <Button
                    onClick={() => handlePair(device)}
                    disabled={pairingIndex === device.index || pairedIndices.has(device.index)}
                    variant={pairedIndices.has(device.index) ? 'outline' : 'default'}
                  >
                    {pairingIndex === device.index && (
                      <Loader2 className="mr-2 size-4 animate-spin" />
                    )}
                    {pairedIndices.has(device.index) ? 'Paired' : pairingIndex === device.index ? 'Pairing...' : 'Pair'}
                  </Button>
                </div>
              ))
            )}
          </div>
        </CardContent>
      </Card>

      {/* Paired Hosts */}
      {pairedHosts.length > 0 && (
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle>Paired Hosts</CardTitle>
                <CardDescription>Previously paired SSH connections</CardDescription>
              </div>
              <Badge variant="secondary">{pairedHosts.length} host(s)</Badge>
            </div>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              {pairedHosts.map((host) => (
                <div
                  key={host.host}
                  className="flex items-center justify-between p-4 border rounded-lg hover:bg-gray-50 transition-colors"
                >
                  <div className="flex items-center gap-4">
                    <div className="p-2 bg-green-100 rounded-lg">
                      <Link2 className="size-5 text-green-700" />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <p className="font-medium">{host.host}</p>
                        <Badge variant="outline" className="text-green-600 border-green-300">
                          <CheckCircle2 className="mr-1 size-3" />
                          Paired
                        </Badge>
                      </div>
                      <p className="text-sm text-gray-500">
                        {host.user}@{host.hostname}
                      </p>
                    </div>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => copyToClipboard(`ssh ${host.host}`)}
                  >
                    <Copy className="mr-2 size-3" />
                    Copy SSH
                  </Button>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      {/* Manual Connect */}
      <Card>
        <CardHeader>
          <CardTitle>Manual Connect</CardTitle>
          <CardDescription>Enter IP:port directly if mDNS doesn't find the device</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex gap-2">
            <Input
              placeholder="192.168.1.100:8099"
              value={manualIp}
              onChange={(e) => setManualIp(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleManualConnect()}
            />
            <Button onClick={handleManualConnect}>Connect</Button>
          </div>
        </CardContent>
      </Card>

      {/* SSH Command Result */}
      {pairingResult && pairingResult.success && (
        <Card className="border-green-200 bg-green-50">
          <CardHeader>
            <CardTitle className="text-green-900">Connection Ready!</CardTitle>
            <CardDescription className="text-green-700">
              Successfully paired with {pairingResult.server_name}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <div>
              <p className="text-sm font-medium text-green-800 mb-1">SSH Command</p>
              <div className="flex items-center gap-2 p-3 bg-white border rounded-lg font-mono text-sm">
                <code className="flex-1">{pairingResult.ssh_command}</code>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => copyToClipboard(pairingResult.ssh_command)}
                >
                  <Copy className="size-4" />
                </Button>
              </div>
            </div>
            <div>
              <p className="text-sm font-medium text-green-800 mb-1">Private Key</p>
              <div className="flex items-center gap-2 p-3 bg-white border rounded-lg font-mono text-sm">
                <code className="flex-1 truncate">{pairingResult.private_key_path}</code>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => copyToClipboard(pairingResult.private_key_path)}
                >
                  <Copy className="size-4" />
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
