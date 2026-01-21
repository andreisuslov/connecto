import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/app/components/ui/card';
import { Button } from '@/app/components/ui/button';
import { Input } from '@/app/components/ui/input';
import { Badge } from '@/app/components/ui/badge';
import { Checkbox } from '@/app/components/ui/checkbox';
import { Key, Trash2, RefreshCw, Loader2, Plus, CircleHelp, Pencil, Copy } from 'lucide-react';
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/app/components/ui/dialog';
import { TooltipProvider, TooltipTrigger } from '@radix-ui/react-tooltip';
import { Tooltip, TooltipContent } from './ui/tooltip';

interface ParsedKey {
  type: string;
  data: string;
  comment: string;
  raw: string;
}

interface LocalKeyInfo {
  name: string;
  algorithm: string;
  comment: string;
  private_key_path: string;
  public_key_path: string;
  fingerprint: string;
  created: string | null;
}

export function KeysTab() {
  const [keys, setKeys] = useState<ParsedKey[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [keyName, setKeyName] = useState('');
  const [keyComment, setKeyComment] = useState('');
  const [useRsa, setUseRsa] = useState(false);
  const [isGenerating, setIsGenerating] = useState(false);
  const [generatedKey, setGeneratedKey] = useState<{ privatePath: string; publicPath: string } | null>(null);

  // Local keys state
  const [localKeys, setLocalKeys] = useState<LocalKeyInfo[]>([]);
  const [isLoadingLocal, setIsLoadingLocal] = useState(true);
  const [renameDialogOpen, setRenameDialogOpen] = useState(false);
  const [keyToRename, setKeyToRename] = useState<LocalKeyInfo | null>(null);
  const [newKeyName, setNewKeyName] = useState('');

  useEffect(() => {
    loadKeys();
    loadLocalKeys();
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

  // Local keys functions
  const loadLocalKeys = async () => {
    setIsLoadingLocal(true);
    try {
      const keys = await invoke<LocalKeyInfo[]>('list_local_keys');
      setLocalKeys(keys);
    } catch (error) {
      toast.error(`Failed to load local keys: ${error}`);
    } finally {
      setIsLoadingLocal(false);
    }
  };

  const handleDeleteLocalKey = async (key: LocalKeyInfo) => {
    try {
      await invoke('delete_local_key', { name: key.name });
      toast.success(`Key "${key.name}" deleted`);
      loadLocalKeys();
    } catch (error) {
      toast.error(`Failed to delete key: ${error}`);
    }
  };

  const handleRenameKey = async () => {
    if (!keyToRename || !newKeyName.trim()) return;

    if (!/^[a-zA-Z0-9_-]+$/.test(newKeyName)) {
      toast.error('Key name can only contain letters, numbers, underscores, and hyphens');
      return;
    }

    try {
      await invoke('rename_local_key', { oldName: keyToRename.name, newName: newKeyName });
      toast.success(`Key renamed to "${newKeyName}"`);
      setRenameDialogOpen(false);
      setKeyToRename(null);
      setNewKeyName('');
      loadLocalKeys();
    } catch (error) {
      toast.error(`Failed to rename key: ${error}`);
    }
  };

  const copyToClipboard = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success('Copied to clipboard');
    } catch {
      toast.error('Failed to copy');
    }
  };

  const truncateFingerprint = (fingerprint: string) => {
    if (fingerprint.length <= 30) return fingerprint;
    return `${fingerprint.substring(0, 20)}...`;
  };

  const renderAuthorizedKeysContent = () => {
    if (isLoading) {
      return (
        <div className="flex items-center justify-center py-8">
          <Loader2 className="size-6 animate-spin text-gray-400" />
        </div>
      );
    }
    if (keys.length === 0) {
      return (
        <p className="text-center text-gray-500 py-8">
          No authorized keys found
        </p>
      );
    }
    return (
      <div className="space-y-3">
        {keys.map((key) => (
          <div
            key={key.data}
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
    );
  };

  const renderLocalKeysContent = () => {
    if (isLoadingLocal) {
      return (
        <div className="flex items-center justify-center py-8">
          <Loader2 className="size-6 animate-spin text-gray-400" />
        </div>
      );
    }
    if (localKeys.length === 0) {
      return (
        <p className="text-center text-gray-500 py-8">
          No local keys found in ~/.ssh
        </p>
      );
    }
    return (
      <div className="space-y-3">
        {localKeys.map((key) => (
          <div
            key={key.name}
            className="flex items-start justify-between p-4 border rounded-lg hover:bg-gray-50 transition-colors"
          >
            <div className="flex items-start gap-4">
              <div className="p-2 bg-blue-100 rounded-lg">
                <Key className="size-5 text-blue-600" />
              </div>
              <div className="min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  <span className="font-medium">{key.name}</span>
                  <Badge variant="secondary" className="font-mono text-xs">
                    {key.algorithm}
                  </Badge>
                </div>
                {key.comment && (
                  <p className="text-sm text-gray-600">{key.comment}</p>
                )}
                <div className="flex items-center gap-2 mt-1">
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <button
                          type="button"
                          className="text-xs text-gray-400 font-mono cursor-pointer hover:text-gray-600 bg-transparent border-none p-0"
                          onClick={() => copyToClipboard(key.fingerprint)}
                        >
                          {truncateFingerprint(key.fingerprint)}
                        </button>
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Click to copy fingerprint</p>
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </div>
                <p className="text-xs text-gray-400 mt-1">
                  {key.private_key_path}
                </p>
              </div>
            </div>
            <div className="flex items-center gap-1">
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => copyToClipboard(key.public_key_path)}
                    >
                      <Copy className="size-4" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>Copy public key path</p>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        setKeyToRename(key);
                        setNewKeyName(key.name);
                        setRenameDialogOpen(true);
                      }}
                    >
                      <Pencil className="size-4" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>Rename key</p>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button variant="ghost" size="sm" className="text-red-600 hover:text-red-700 hover:bg-red-50">
                    <Trash2 className="size-4" />
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Delete SSH key pair</AlertDialogTitle>
                    <AlertDialogDescription>
                      This will permanently delete both the private and public key files for "{key.name}".
                      Any services using this key will lose access. This action cannot be undone.
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction
                      onClick={() => handleDeleteLocalKey(key)}
                      className="bg-red-600 hover:bg-red-700"
                    >
                      Delete
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            </div>
          </div>
        ))}
      </div>
    );
  };

  return (
    <div className="space-y-6">
      {/* Authorized keys */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Authorized keys</CardTitle>
              <CardDescription>SSH keys authorized to connect to this machine</CardDescription>
            </div>
            <Button variant="outline" size="sm" onClick={loadKeys} disabled={isLoading}>
              <RefreshCw className={`mr-2 size-4 ${isLoading ? 'animate-spin' : ''}`} />
              Refresh
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          {renderAuthorizedKeysContent()}
        </CardContent>
      </Card>

      {/* Local keys */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Local keys</CardTitle>
              <CardDescription>SSH key pairs stored on this machine</CardDescription>
            </div>
            <Button variant="outline" size="sm" onClick={loadLocalKeys} disabled={isLoadingLocal}>
              <RefreshCw className={`mr-2 size-4 ${isLoadingLocal ? 'animate-spin' : ''}`} />
              Refresh
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          {renderLocalKeysContent()}
        </CardContent>
      </Card>

      {/* Rename dialog */}
      <Dialog open={renameDialogOpen} onOpenChange={setRenameDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Rename key</DialogTitle>
            <DialogDescription>
              Enter a new name for the key "{keyToRename?.name}"
            </DialogDescription>
          </DialogHeader>
          <div className="py-4">
            <Input
              value={newKeyName}
              onChange={(e) => setNewKeyName(e.target.value)}
              placeholder="new_key_name"
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRenameDialogOpen(false)}>
              Cancel
            </Button>
            <Button onClick={handleRenameKey}>
              Rename
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Generate new key */}
      <Card>
        <CardHeader>
          <CardTitle>Generate new key</CardTitle>
          <CardDescription>Create a new SSH key pair for this machine</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label htmlFor="keyName" className="text-sm font-medium mb-2 block">Key name</label>
              <Input
                id="keyName"
                value={keyName}
                onChange={(e) => setKeyName(e.target.value)}
                placeholder="my_key"
              />
            </div>
            <div>
              <label htmlFor="keyComment" className="text-sm font-medium mb-2 block">Comment (optional)</label>
              <Input
                id="keyComment"
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
              <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <CircleHelp className="size-4 text-muted-foreground hover:text-foreground cursor-help transition-colors" />
                </TooltipTrigger>
                <TooltipContent>
                  <p className="max-w-xs">
                    Why?{' '}
                    <a
                      href="https://andreisuslov.github.io/connecto/reference/security.html"
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
                Generate key
              </>
            )}
          </Button>

          {generatedKey && (
            <div className="mt-4 p-4 bg-green-50 border border-green-200 rounded-lg">
              <p className="text-green-800 font-medium mb-2">Key generated successfully</p>
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
