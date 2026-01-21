import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/app/components/ui/card';
import { Button } from '@/app/components/ui/button';
import { Input } from '@/app/components/ui/input';
import { Badge } from '@/app/components/ui/badge';
import { Checkbox } from '@/app/components/ui/checkbox';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/app/components/ui/tooltip";
import { RefreshCw, Loader2, Copy, StopCircle, CircleHelp, CheckCircle2, XCircle } from 'lucide-react';
import { toast } from 'sonner';

interface SyncResult {
  success: boolean;
  peer_name: string;
  peer_user: string;
  peer_address: string;
  ssh_command: string;
  error: string | null;
}

interface SyncStatus {
  is_syncing: boolean;
  status_message: string;
  peer_name: string | null;
}

export function SyncTab() {
  const [isSyncing, setIsSyncing] = useState(false);
  const [deviceName, setDeviceName] = useState('');
  const [port, setPort] = useState('8099');
  const [timeout, setTimeout] = useState('60');
  const [useRsa, setUseRsa] = useState(false);
  const [addresses, setAddresses] = useState<string[]>([]);
  const [syncResult, setSyncResult] = useState<SyncResult | null>(null);
  const [statusMessage, setStatusMessage] = useState('');

  useEffect(() => {
    loadInitialData();
  }, []);

  const loadInitialData = async () => {
    try {
      const [name, addrs] = await Promise.all([
        invoke<string>('get_device_name'),
        invoke<string[]>('get_addresses')
      ]);
      setDeviceName(name);
      setAddresses(addrs);
    } catch (error) {
      console.error('Failed to load initial data:', error);
    }
  };

  const handleStartSync = async () => {
    setIsSyncing(true);
    setSyncResult(null);
    setStatusMessage('Starting sync...');

    try {
      const result = await invoke<SyncResult>('start_sync', {
        port: Number.parseInt(port, 10),
        deviceName: deviceName || null,
        timeoutSecs: Number.parseInt(timeout, 10),
        useRsa
      });

      setSyncResult(result);

      if (result.success) {
        toast.success(`Synced with ${result.peer_name}!`);
        setStatusMessage(`Sync completed with ${result.peer_name}`);
      } else {
        toast.error(`Sync failed: ${result.error}`);
        setStatusMessage(result.error || 'Sync failed');
      }
    } catch (error) {
      toast.error(`Sync error: ${error}`);
      setStatusMessage(`Error: ${error}`);
    } finally {
      setIsSyncing(false);
    }
  };

  const handleCancelSync = async () => {
    try {
      await invoke('cancel_sync');
      setIsSyncing(false);
      setStatusMessage('Sync cancelled');
      toast.info('Sync cancelled');
    } catch (error) {
      toast.error(`Failed to cancel sync: ${error}`);
    }
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    toast.success('Copied to clipboard!');
  };

  return (
    <div className="space-y-6">
      {/* Syncing status */}
      {isSyncing && (
        <Card className="border-purple-200 bg-purple-50">
          <CardHeader>
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <div className="relative">
                  <RefreshCw className="size-5 text-purple-600 animate-spin" />
                </div>
                <div>
                  <CardTitle className="text-purple-900">Syncing...</CardTitle>
                  <CardDescription className="text-purple-700">
                    {statusMessage || 'Searching for sync peer on the network'}
                  </CardDescription>
                </div>
              </div>
              <Button variant="destructive" onClick={handleCancelSync}>
                <StopCircle className="mr-2 size-4" />
                Cancel
              </Button>
            </div>
          </CardHeader>
        </Card>
      )}

      {/* Sync result */}
      {syncResult && !isSyncing && (
        <Card className={syncResult.success ? "border-green-200 bg-green-50" : "border-red-200 bg-red-50"}>
          <CardHeader>
            <div className="flex items-center gap-3">
              {syncResult.success ? (
                <CheckCircle2 className="size-6 text-green-600" />
              ) : (
                <XCircle className="size-6 text-red-600" />
              )}
              <div>
                <CardTitle className={syncResult.success ? "text-green-900" : "text-red-900"}>
                  {syncResult.success ? `Synced with ${syncResult.peer_name}` : 'Sync Failed'}
                </CardTitle>
                <CardDescription className={syncResult.success ? "text-green-700" : "text-red-700"}>
                  {syncResult.success
                    ? `Bidirectional SSH access established with ${syncResult.peer_user}@${syncResult.peer_address}`
                    : syncResult.error}
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          {syncResult.success && (
            <CardContent>
              <div className="space-y-2">
                <p className="text-sm font-medium text-green-800">SSH Command:</p>
                <div
                  className="flex items-center justify-between bg-white rounded-md px-3 py-2 border border-green-200 cursor-pointer hover:bg-green-100 transition-colors"
                  onClick={() => copyToClipboard(syncResult.ssh_command)}
                >
                  <code className="text-sm font-mono text-green-900">{syncResult.ssh_command}</code>
                  <Copy className="size-4 text-green-600" />
                </div>
              </div>
            </CardContent>
          )}
        </Card>
      )}

      {/* Description */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            Bidirectional Sync
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <CircleHelp className="size-4 text-muted-foreground hover:text-foreground cursor-help transition-colors" />
                </TooltipTrigger>
                <TooltipContent>
                  <p className="max-w-xs">
                    Sync exchanges SSH keys between two devices simultaneously, so both can SSH to each other.
                  </p>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </CardTitle>
          <CardDescription>
            Run <code className="bg-gray-100 px-1 rounded">connecto sync</code> on two devices at the same time.
            Both devices will exchange SSH keys and can SSH to each other after sync completes.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-3 gap-4">
            <div>
              <label htmlFor="syncDeviceName" className="text-sm font-medium mb-2 block">Device name</label>
              <Input
                id="syncDeviceName"
                value={deviceName}
                onChange={(e) => setDeviceName(e.target.value)}
                placeholder="My computer"
                disabled={isSyncing}
              />
            </div>
            <div>
              <label htmlFor="syncPort" className="text-sm font-medium mb-2 block">Port</label>
              <Input
                id="syncPort"
                type="number"
                value={port}
                onChange={(e) => setPort(e.target.value)}
                placeholder="8099"
                disabled={isSyncing}
              />
            </div>
            <div>
              <label htmlFor="syncTimeout" className="text-sm font-medium mb-2 block">Timeout (seconds)</label>
              <Input
                id="syncTimeout"
                type="number"
                value={timeout}
                onChange={(e) => setTimeout(e.target.value)}
                placeholder="60"
                disabled={isSyncing}
              />
            </div>
          </div>

          <div className="flex items-center space-x-2">
            <Checkbox
              id="syncUseRsa"
              checked={useRsa}
              onCheckedChange={(checked) => setUseRsa(checked as boolean)}
              disabled={isSyncing}
            />
            <label
              htmlFor="syncUseRsa"
              className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
            >
              Use RSA-4096 instead of Ed25519
            </label>
          </div>

          {!isSyncing && (
            <Button onClick={handleStartSync} className="w-full bg-purple-600 hover:bg-purple-700">
              <RefreshCw className="mr-2 size-4" />
              Start Sync
            </Button>
          )}
        </CardContent>
      </Card>

      {/* Network information */}
      <Card>
        <CardHeader>
          <CardTitle>Your IP Addresses</CardTitle>
          <CardDescription>Other device needs to be on the same network to sync</CardDescription>
        </CardHeader>
        <CardContent>
          {addresses.length === 0 ? (
            <p className="text-gray-500 text-center py-4">No network addresses found</p>
          ) : (
            <div className="flex flex-wrap gap-2">
              {addresses.map((addr) => (
                <Badge
                  key={addr}
                  variant="secondary"
                  className="cursor-pointer hover:bg-gray-200 transition-colors font-mono"
                  onClick={() => copyToClipboard(addr)}
                >
                  {addr}
                  <Copy className="ml-2 size-3" />
                </Badge>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* How it works */}
      <Card>
        <CardHeader>
          <CardTitle>How Sync Works</CardTitle>
        </CardHeader>
        <CardContent>
          <ol className="list-decimal list-inside space-y-2 text-sm text-muted-foreground">
            <li>Run <code className="bg-gray-100 px-1 rounded">connecto sync</code> on both devices</li>
            <li>Both devices advertise via mDNS and search for each other</li>
            <li>When found, they exchange SSH public keys</li>
            <li>Both devices add each other's key to <code className="bg-gray-100 px-1 rounded">~/.ssh/authorized_keys</code></li>
            <li>After sync, both devices can SSH to each other without passwords</li>
          </ol>
        </CardContent>
      </Card>
    </div>
  );
}
