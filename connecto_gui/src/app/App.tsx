import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/app/components/ui/tabs';
import { ScanAndPairTab } from '@/app/components/ScanAndPairTab';
import { ListenTab } from '@/app/components/ListenTab';
import { KeysTab } from '@/app/components/KeysTab';
import { Toaster } from '@/app/components/ui/sonner';
import { Radio } from 'lucide-react';

export default function App() {
  return (
    <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100">
      <Toaster />

      {/* Header */}
      <div className="border-b bg-white shadow-sm">
        <div className="max-w-4xl mx-auto px-6 py-4">
          <div className="flex items-center gap-3">
            <div className="p-2 bg-gradient-to-br from-blue-500 to-purple-600 rounded-xl">
              <Radio className="size-5 text-white" />
            </div>
            <h1 className="text-xl font-bold">Connecto</h1>
          </div>
        </div>
      </div>

      {/* Main Content */}
      <div className="max-w-4xl mx-auto px-6 py-6">
        <Tabs defaultValue="scan" className="space-y-6">
          <TabsList className="grid w-full grid-cols-3 max-w-md">
            <TabsTrigger value="scan">Scan & Pair</TabsTrigger>
            <TabsTrigger value="listen">Listen</TabsTrigger>
            <TabsTrigger value="keys">Keys</TabsTrigger>
          </TabsList>

          <TabsContent value="scan" className="space-y-6">
            <ScanAndPairTab />
          </TabsContent>

          <TabsContent value="listen" className="space-y-6">
            <ListenTab />
          </TabsContent>

          <TabsContent value="keys" className="space-y-6">
            <KeysTab />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
