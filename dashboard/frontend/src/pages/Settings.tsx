import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getAiSettings, setAiSettings, deleteAiSettings } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { BrainCircuit } from 'lucide-react';
import { toast } from 'sonner';

const PROVIDERS = [
  {
    value: 'anthropic',
    label: 'Anthropic (direct)',
    description: 'Use your API key from console.anthropic.com',
    link: 'https://console.anthropic.com/settings/keys',
    linkLabel: 'Get an Anthropic API key',
  },
  {
    value: 'openrouter',
    label: 'OpenRouter',
    description: 'Access Claude and other models via openrouter.ai',
    link: 'https://openrouter.ai/keys',
    linkLabel: 'Get an OpenRouter API key',
  },
] as const;

type ProviderValue = (typeof PROVIDERS)[number]['value'];

export default function Settings() {
  const queryClient = useQueryClient();
  const { data: settings, isLoading } = useQuery({
    queryKey: ['ai-settings'],
    queryFn: getAiSettings,
  });

  const [selectedProvider, setSelectedProvider] = useState<ProviderValue>('anthropic');
  const [apiKey, setApiKey] = useState('');

  const saveMutation = useMutation({
    mutationFn: (data: { provider: string; api_key: string }) => setAiSettings(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['ai-settings'] });
      setApiKey('');
      toast.success('AI provider saved');
    },
    onError: (e: Error) => toast.error(e.message),
  });

  const deleteMutation = useMutation({
    mutationFn: deleteAiSettings,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['ai-settings'] });
      toast.success('AI provider disconnected');
    },
    onError: (e: Error) => toast.error(e.message),
  });

  const currentProvider = PROVIDERS.find((p) => p.value === selectedProvider)!;

  const handleSave = () => {
    if (!apiKey.trim()) {
      toast.error('API key cannot be empty');
      return;
    }
    saveMutation.mutate({ provider: selectedProvider, api_key: apiKey.trim() });
  };

  return (
    <div className="space-y-6 max-w-2xl">
      <h1 className="text-2xl font-bold">Settings</h1>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <BrainCircuit className="size-5" />
            AI Provider
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-6">
          {isLoading ? (
            <p className="text-muted-foreground text-sm">Loading...</p>
          ) : (
            <>
              {settings?.has_api_key && (
                <div className="flex items-center justify-between rounded-md border px-4 py-3">
                  <div className="text-sm">
                    <span className="text-muted-foreground">Connected provider: </span>
                    <span className="font-medium capitalize">
                      {PROVIDERS.find((p) => p.value === settings.provider)?.label ??
                        settings.provider}
                    </span>
                    <span className="text-muted-foreground ml-2">(API key stored)</span>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => deleteMutation.mutate()}
                    disabled={deleteMutation.isPending}
                  >
                    Disconnect
                  </Button>
                </div>
              )}

              <div className="space-y-3">
                <Label>Provider</Label>
                <div className="space-y-2">
                  {PROVIDERS.map((p) => (
                    <label
                      key={p.value}
                      className="flex cursor-pointer items-start gap-3 rounded-md border px-4 py-3 hover:bg-muted/50"
                    >
                      <input
                        type="radio"
                        name="provider"
                        value={p.value}
                        checked={selectedProvider === p.value}
                        onChange={() => setSelectedProvider(p.value)}
                        className="mt-0.5"
                      />
                      <div className="space-y-0.5">
                        <div className="text-sm font-medium">{p.label}</div>
                        <div className="text-xs text-muted-foreground">
                          {p.description} —{' '}
                          <a
                            href={p.link}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="underline underline-offset-2 hover:text-foreground"
                          >
                            {p.linkLabel}
                          </a>
                        </div>
                      </div>
                    </label>
                  ))}
                </div>
              </div>

              <div className="space-y-2">
                <Label htmlFor="api-key">
                  API Key{settings?.has_api_key && ' (leave blank to keep existing)'}
                </Label>
                <Input
                  id="api-key"
                  type="password"
                  placeholder={
                    settings?.has_api_key ? '••••••••••••••••' : currentProvider.linkLabel
                  }
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  autoComplete="off"
                />
              </div>

              <Button
                onClick={handleSave}
                disabled={saveMutation.isPending || (!apiKey.trim() && !settings?.has_api_key)}
              >
                {saveMutation.isPending ? 'Saving...' : settings?.has_api_key ? 'Update' : 'Save'}
              </Button>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
