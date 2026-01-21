import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/app/components/ui/card';
import { Button } from '@/app/components/ui/button';
import { Input } from '@/app/components/ui/input';
import { Badge } from '@/app/components/ui/badge';
import { Checkbox } from '@/app/components/ui/checkbox';
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/app/components/ui/accordion";
import { Wifi, Loader2, CheckCircle2, Monitor, Copy, Link2, RefreshCw, StopCircle, XCircle } from 'lucide-react';
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

interface SyncResult {
  success: boolean;
  peer_name: string;
  peer_user: string;
  peer_address: string;
  ssh_command: string;
  error: string | null;
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

  // Sync state
  const [isSyncing, setIsSyncing] = useState(false);
  const [syncTimeout, setSyncTimeout] = useState('60');
  const [syncUseRsa, setSyncUseRsa] = useState(false);
  const [syncResult, setSyncResult] = useState<SyncResult | null>(null);

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
        loadPairedHosts();
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
        loadPairedHosts();
        toast.success(`Successfully paired!`, { id: 'manual' });
        setManualIp('');
      } else {
        toast.error(`Connection failed: ${result.error}`, { id: 'manual' });
      }
    } catch (error) {
      toast.error(`Connection failed: ${error}`, { id: 'manual' });
    }
  };

  const handleStartSync = async () => {
    setIsSyncing(true);
    setSyncResult(null);
    toast.loading('Starting sync - waiting for peer...', { id: 'sync' });

    try {
      const result = await invoke<SyncResult>('start_sync', {
        port: 8099,
        deviceName: null,
        timeoutSecs: Number.parseInt(syncTimeout, 10),
        useRsa: syncUseRsa
      });

      setSyncResult(result);

      if (result.success) {
        toast.success(`Synced with ${result.peer_name}!`, { id: 'sync' });
        loadPairedHosts();
      } else {
        toast.error(`Sync failed: ${result.error}`, { id: 'sync' });
      }
    } catch (error) {
      toast.error(`Sync error: ${error}`, { id: 'sync' });
    } finally {
      setIsSyncing(false);
    }
  };

  const handleCancelSync = async () => {
    try {
      await invoke('cancel_sync');
      setIsSyncing(false);
      toast.info('Sync cancelled', { id: 'sync' });
    } catch (error) {
      toast.error(`Failed to cancel sync: ${error}`);
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
      {/* Network discovery */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Network discovery</CardTitle>
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
                  Scan network
                </>
              )}
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          <div className="space-y-3">
            {devices.length === 0 ? (
              <p className="text-center text-gray-500 py-8">
                No devices found. Click "Scan network" to search.
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

      {/* Paired hosts */}
      {pairedHosts.length > 0 && (
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle>Paired hosts</CardTitle>
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

      {/* Manual connect and Sync */}
      <Card>
        <CardHeader>
          <CardTitle>Additional options</CardTitle>
          <CardDescription>Manual connections and bidirectional sync</CardDescription>
        </CardHeader>
        <CardContent>
          <Accordion type="single" collapsible className="w-full">
            <AccordionItem value="manual">
              <AccordionTrigger>
                <div className="flex items-center gap-2">
                  <Link2 className="size-4" />
                  Manual connect
                </div>
              </AccordionTrigger>
              <AccordionContent>
                <p className="text-sm text-muted-foreground mb-4">
                  Enter IP:port directly if mDNS doesn't find the device
                </p>
                <div className="flex gap-2">
                  <Input
                    placeholder="192.168.1.100:8099"
                    value={manualIp}
                    onChange={(e) => setManualIp(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && handleManualConnect()}
                  />
                  <Button onClick={handleManualConnect}>Connect</Button>
                </div>
              </AccordionContent>
            </AccordionItem>

            <AccordionItem value="sync">
              <AccordionTrigger>
                <div className="flex items-center gap-2">
                  <RefreshCw className="size-4" />
                  Bidirectional sync
                </div>
              </AccordionTrigger>
              <AccordionContent>
                <p className="text-sm text-muted-foreground mb-4">
                  Run sync on both devices simultaneously. After sync, both can SSH to each other.
                </p>

                {/* Sync status */}
                {isSyncing && (
                  <div className="mb-4 p-4 bg-purple-50 border border-purple-200 rounded-lg">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <RefreshCw className="size-5 text-purple-600 animate-spin" />
                        <div>
                          <p className="font-medium text-purple-900">Syncing...</p>
                          <p className="text-sm text-purple-700">Waiting for peer on network</p>
                        </div>
                      </div>
                      <Button variant="destructive" size="sm" onClick={handleCancelSync}>
                        <StopCircle className="mr-2 size-4" />
                        Cancel
                      </Button>
                    </div>
                  </div>
                )}

                {/* Sync result */}
                {syncResult && !isSyncing && (
                  <div className={`mb-4 p-4 rounded-lg border ${syncResult.success ? 'bg-green-50 border-green-200' : 'bg-red-50 border-red-200'}`}>
                    <div className="flex items-center gap-3">
                      {syncResult.success ? (
                        <CheckCircle2 className="size-5 text-green-600" />
                      ) : (
                        <XCircle className="size-5 text-red-600" />
                      )}
                      <div className="flex-1">
                        <p className={`font-medium ${syncResult.success ? 'text-green-900' : 'text-red-900'}`}>
                          {syncResult.success ? `Synced with ${syncResult.peer_name}` : 'Sync failed'}
                        </p>
                        {syncResult.success ? (
                          <p className="text-sm text-green-700">
                            Bidirectional SSH access with {syncResult.peer_user}@{syncResult.peer_address}
                          </p>
                        ) : (
                          <p className="text-sm text-red-700">{syncResult.error}</p>
                        )}
                      </div>
                      {syncResult.success && (
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => copyToClipboard(syncResult.ssh_command)}
                        >
                          <Copy className="mr-2 size-3" />
                          Copy SSH
                        </Button>
                      )}
                    </div>
                  </div>
                )}

                <div className="space-y-4">
                  <div className="flex items-center gap-4">
                    <div className="flex-1">
                      <label htmlFor="syncTimeout" className="text-sm font-medium mb-2 block">
                        Timeout (seconds)
                      </label>
                      <Input
                        id="syncTimeout"
                        type="number"
                        value={syncTimeout}
                        onChange={(e) => setSyncTimeout(e.target.value)}
                        disabled={isSyncing}
                        className="w-32"
                      />
                    </div>
                    <div className="flex items-center space-x-2 pt-6">
                      <Checkbox
                        id="syncUseRsa"
                        checked={syncUseRsa}
                        onCheckedChange={(checked) => setSyncUseRsa(checked as boolean)}
                        disabled={isSyncing}
                      />
                      <label
                        htmlFor="syncUseRsa"
                        className="text-sm font-medium leading-none"
                      >
                        Use RSA-4096
                      </label>
                    </div>
                  </div>

                  {!isSyncing && (
                    <Button onClick={handleStartSync} className="w-full bg-purple-600 hover:bg-purple-700">
                      <RefreshCw className="mr-2 size-4" />
                      Start Sync
                    </Button>
                  )}
                </div>
              </AccordionContent>
            </AccordionItem>
          </Accordion>
        </CardContent>
      </Card>

      {/* SSH command result */}
      {pairingResult && pairingResult.success && (
        <Card className="border-green-200 bg-green-50">
          <CardHeader>
            <CardTitle className="text-green-900">Connection ready!</CardTitle>
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
              <p className="text-sm font-medium text-green-800 mb-1">Private key</p>
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
