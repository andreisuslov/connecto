import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/app/components/ui/card';
import { Button } from '@/app/components/ui/button';
import { Input } from '@/app/components/ui/input';
import { Badge } from '@/app/components/ui/badge';
import { Checkbox } from '@/app/components/ui/checkbox';
import { Key, Trash2, RefreshCw, Loader2, Plus, AlertTriangle } from 'lucide-react';
import { toast } from 'sonner';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/app/components/ui/alert-dialog';

interface ParsedKey {
  type: string;
  data: string;
  comment: string;
  raw: string;
}

export function KeysTab() {
  const [keys, setKeys] = useState<ParsedKey[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [keyName, setKeyName] = useState('');
  const [keyComment, setKeyComment] = useState('');
  const [useRsa, setUseRsa] = useState(false);
  const [isGenerating, setIsGenerating] = useState(false);
  const [generatedKey, setGeneratedKey] = useState<{ privatePath: string; publicPath: string } | null>(null);

  useEffect(() => {
    loadKeys();
  }, []);

  const loadKeys = async () => {
    setIsLoading(true);
    try {
      const rawKeys = await invoke<string[]>('list_authorized_keys');
      const parsed = rawKeys.map(parseKey);
      setKeys(parsed);
    } catch (error) {
      toast.error(`Failed to load keys: ${error}`);
    } finally {
      setIsLoading(false);
    }
  };

  const parseKey = (key: string): ParsedKey => {
    const parts = key.split(/\s+/);
    return {
      type: parts[0] || 'unknown',
      data: parts[1] || '',
      comment: parts.slice(2).join(' ') || 'No comment',
      raw: key
    };
  };

  const handleRemoveKey = async (key: ParsedKey) => {
    try {
      await invoke('remove_authorized_key', { key: key.raw });
      toast.success('Key removed');
      loadKeys();
    } catch (error) {
      toast.error(`Failed to remove key: ${error}`);
    }
  };

  const handleGenerateKey = async () => {
    const name = keyName.trim() || 'connecto_key';

    if (!/^[a-zA-Z0-9_-]+$/.test(name)) {
      toast.error('Key name can only contain letters, numbers, underscores, and hyphens');
      return;
    }

    setIsGenerating(true);

    try {
      const [privatePath, publicPath] = await invoke<[string, string]>('generate_key_pair', {
        name,
        comment: keyComment || null,
        useRsa
      });

      setGeneratedKey({ privatePath, publicPath });
      toast.success('SSH key pair generated');
      setKeyName('');
      setKeyComment('');
      setUseRsa(false);
    } catch (error) {
      toast.error(`Failed to generate key: ${error}`);
    } finally {
      setIsGenerating(false);
    }
  };

  const truncateKey = (data: string) => {
    if (data.length <= 40) return data;
    return `${data.substring(0, 16)}...${data.substring(data.length - 16)}`;
  };

  return (
    <div className="space-y-6">
      {/* Authorized Keys */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Authorized Keys</CardTitle>
              <CardDescription>SSH keys authorized to connect to this machine</CardDescription>
            </div>
            <Button variant="outline" size="sm" onClick={loadKeys} disabled={isLoading}>
              <RefreshCw className={`mr-2 size-4 ${isLoading ? 'animate-spin' : ''}`} />
              Refresh
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="size-6 animate-spin text-gray-400" />
            </div>
          ) : keys.length === 0 ? (
            <p className="text-center text-gray-500 py-8">
              No authorized keys found
            </p>
          ) : (
            <div className="space-y-3">
              {keys.map((key, index) => (
                <div
                  key={index}
                  className="flex items-start justify-between p-4 border rounded-lg hover:bg-gray-50 transition-colors"
                >
                  <div className="flex items-start gap-4">
                    <div className="p-2 bg-purple-100 rounded-lg">
                      <Key className="size-5 text-purple-600" />
                    </div>
                    <div className="min-w-0">
                      <div className="flex items-center gap-2 mb-1">
                        <Badge variant="secondary" className="font-mono text-xs">
                          {key.type}
                        </Badge>
                      </div>
                      <p className="font-medium text-sm">{key.comment}</p>
                      <p className="text-xs text-gray-400 font-mono truncate max-w-md">
                        {truncateKey(key.data)}
                      </p>
                    </div>
                  </div>
                  <AlertDialog>
                    <AlertDialogTrigger asChild>
                      <Button variant="ghost" size="sm" className="text-red-600 hover:text-red-700 hover:bg-red-50">
                        <Trash2 className="size-4" />
                      </Button>
                    </AlertDialogTrigger>
                    <AlertDialogContent>
                      <AlertDialogHeader>
                        <AlertDialogTitle>Remove SSH Key</AlertDialogTitle>
                        <AlertDialogDescription>
                          This will revoke access for any device using this key. This action cannot be undone.
                        </AlertDialogDescription>
                      </AlertDialogHeader>
                      <AlertDialogFooter>
                        <AlertDialogCancel>Cancel</AlertDialogCancel>
                        <AlertDialogAction
                          onClick={() => handleRemoveKey(key)}
                          className="bg-red-600 hover:bg-red-700"
                        >
                          Remove
                        </AlertDialogAction>
                      </AlertDialogFooter>
                    </AlertDialogContent>
                  </AlertDialog>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Generate New Key */}
      <Card>
        <CardHeader>
          <CardTitle>Generate New Key</CardTitle>
          <CardDescription>Create a new SSH key pair for this machine</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="text-sm font-medium mb-2 block">Key Name</label>
              <Input
                value={keyName}
                onChange={(e) => setKeyName(e.target.value)}
                placeholder="my_key"
              />
            </div>
            <div>
              <label className="text-sm font-medium mb-2 block">Comment (optional)</label>
              <Input
                value={keyComment}
                onChange={(e) => setKeyComment(e.target.value)}
                placeholder="user@hostname"
              />
            </div>
          </div>

          <div className="flex items-center space-x-2">
            <Checkbox
              id="useRsa"
              checked={useRsa}
              onCheckedChange={(checked) => setUseRsa(checked as boolean)}
            />
            <label htmlFor="useRsa" className="text-sm text-gray-600 cursor-pointer">
              Use RSA-4096 instead of Ed25519
            </label>
          </div>

          <Button onClick={handleGenerateKey} disabled={isGenerating}>
            {isGenerating ? (
              <>
                <Loader2 className="mr-2 size-4 animate-spin" />
                Generating...
              </>
            ) : (
              <>
                <Plus className="mr-2 size-4" />
                Generate Key
              </>
            )}
          </Button>

          {generatedKey && (
            <div className="mt-4 p-4 bg-green-50 border border-green-200 rounded-lg">
              <p className="text-green-800 font-medium mb-2">Key Generated Successfully</p>
              <div className="space-y-1 text-sm font-mono text-green-700">
                <p>Private: {generatedKey.privatePath}</p>
                <p>Public: {generatedKey.publicPath}</p>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
