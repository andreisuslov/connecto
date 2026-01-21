import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/app/components/ui/card';
import { Button } from '@/app/components/ui/button';
import { Input } from '@/app/components/ui/input';
import { Badge } from '@/app/components/ui/badge';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/app/components/ui/tooltip";
import { Radio, Loader2, Copy, StopCircle, CircleHelp } from 'lucide-react';
import { toast } from 'sonner';

interface ListenerStatus {
  device_name: string;
  port: number;
}

export function ListenTab() {
  const [isListening, setIsListening] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [deviceName, setDeviceName] = useState('');
  const [port, setPort] = useState('8099');
  const [addresses, setAddresses] = useState<string[]>([]);
  const [listenerInfo, setListenerInfo] = useState<ListenerStatus | null>(null);

  useEffect(() => {
    loadInitialData();
    checkListenerStatus();
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

  const checkListenerStatus = async () => {
    try {
      const status = await invoke<boolean>('get_listener_status');
      setIsListening(status);
    } catch (error) {
      console.error('Failed to check listener status:', error);
    }
  };

  const handleStartListening = async () => {
    setIsStarting(true);

    try {
      const status = await invoke<ListenerStatus>('start_listener', {
        port: Number.parseInt(port, 10),
        deviceName: deviceName || null
      });

      setIsListening(true);
      setListenerInfo(status);
      toast.success(`Now listening on port ${status.port}`);
    } catch (error) {
      toast.error(`Failed to start listener: ${error}`);
    } finally {
      setIsStarting(false);
    }
  };

  const handleStopListening = async () => {
    try {
      await invoke('stop_listener');
      setIsListening(false);
      setListenerInfo(null);
      toast.info('Stopped listening');
    } catch (error) {
      toast.error(`Failed to stop listener: ${error}`);
    }
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    toast.success('Copied to clipboard!');
  };

  return (
    <div className="space-y-6">
      {/* Listener status */}
      {isListening && (
        <Card className="border-green-200 bg-green-50">
          <CardHeader>
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <div className="relative">
                  <Radio className="size-5 text-green-600" />
                  <span className="absolute -top-1 -right-1 size-3 bg-green-500 rounded-full animate-pulse" />
                </div>
                <div>
                  <CardTitle className="text-green-900">Listening for Connections</CardTitle>
                  <CardDescription className="text-green-700">
                    {listenerInfo ? `${listenerInfo.device_name} on port ${listenerInfo.port}` : `Port ${port}`}
                  </CardDescription>
                </div>
              </div>
              <Button variant="destructive" onClick={handleStopListening}>
                <StopCircle className="mr-2 size-4" />
                Stop
              </Button>
            </div>
          </CardHeader>
        </Card>
      )}

      {/* Configuration */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            Listener configuration
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <CircleHelp className="size-4 text-muted-foreground hover:text-foreground cursor-help transition-colors" />
                </TooltipTrigger>
                <TooltipContent>
                  <p className="max-w-xs">
                    Need help?{' '}
                    <a
                      href="https://andreisuslov.github.io/connecto/commands/listen.html"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="underline font-medium hover:text-blue-500"
                    >
                      See docs
                    </a>
                  </p>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </CardTitle>
          <CardDescription>Configure how other devices will find and connect to this machine</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label htmlFor="deviceName" className="text-sm font-medium mb-2 block">Device name</label>
              <Input
                id="deviceName"
                value={deviceName}
                onChange={(e) => setDeviceName(e.target.value)}
                placeholder="My computer"
                disabled={isListening}
              />
            </div>
            <div>
              <label htmlFor="port" className="text-sm font-medium mb-2 block">Port</label>
              <Input
                id="port"
                type="number"
                value={port}
                onChange={(e) => setPort(e.target.value)}
                placeholder="8099"
                disabled={isListening}
              />
            </div>
          </div>

          {!isListening && (
            <Button onClick={handleStartListening} disabled={isStarting} className="w-full">
              {isStarting ? (
                <>
                  <Loader2 className="mr-2 size-4 animate-spin" />
                  Starting...
                </>
              ) : (
                <>
                  <Radio className="mr-2 size-4" />
                  Start listening
                </>
              )}
            </Button>
          )}
        </CardContent>
      </Card>

      {/* Network information */}
      <Card>
        <CardHeader>
          <CardTitle>Your IP Addresses</CardTitle>
          <CardDescription>Share one of these with the device you want to pair</CardDescription>
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
    </div>
  );
}
