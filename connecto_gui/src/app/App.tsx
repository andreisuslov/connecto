import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/app/components/ui/tabs';
import { ScanAndPairTab } from '@/app/components/ScanAndPairTab';
import { ListenTab } from '@/app/components/ListenTab';
import { KeysTab } from '@/app/components/KeysTab';
import { SyncTab } from '@/app/components/SyncTab';
import { Toaster } from '@/app/components/ui/sonner';

export default function App() {
  return (
    <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 overflow-x-hidden max-w-full">
      <Toaster />

      {/* Main Content */}
      <div className="max-w-4xl mx-auto px-6 py-6">
        <Tabs defaultValue="scan" className="space-y-6">
          <TabsList className="grid w-full grid-cols-4 max-w-lg">
            <TabsTrigger value="scan">Scan & Pair</TabsTrigger>
            <TabsTrigger value="listen">Listen</TabsTrigger>
            <TabsTrigger value="sync">Sync</TabsTrigger>
            <TabsTrigger value="keys">Keys</TabsTrigger>
          </TabsList>

          <TabsContent value="scan" className="space-y-6">
            <ScanAndPairTab />
          </TabsContent>

          <TabsContent value="listen" className="space-y-6">
            <ListenTab />
          </TabsContent>

          <TabsContent value="sync" className="space-y-6">
            <SyncTab />
          </TabsContent>

          <TabsContent value="keys" className="space-y-6">
            <KeysTab />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
