import { useState, useEffect } from 'react';
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

const OPENROUTER_MODELS = [
  { value: 'anthropic/claude-sonnet-4-6', label: 'Claude Sonnet 4.6 (recommended)' },
  { value: 'anthropic/claude-opus-4-6', label: 'Claude Opus 4.6' },
  { value: 'openai/gpt-4o', label: 'GPT-4o' },
  { value: 'openai/o3', label: 'o3' },
  { value: 'google/gemini-2.0-flash', label: 'Gemini 2.0 Flash' },
  { value: 'google/gemini-2.5-pro', label: 'Gemini 2.5 Pro' },
  { value: 'meta-llama/llama-3.3-70b-instruct', label: 'Llama 3.3 70B' },
  { value: 'deepseek/deepseek-r1', label: 'DeepSeek R1' },
  { value: 'moonshotai/kimi-k2', label: 'Kimi K2' },
];

export default function Settings() {
  const queryClient = useQueryClient();
  const { data: settings, isLoading } = useQuery({
    queryKey: ['ai-settings'],
    queryFn: getAiSettings,
  });

  const [selectedProvider, setSelectedProvider] = useState<ProviderValue>('anthropic');
  const [selectedModel, setSelectedModel] = useState(OPENROUTER_MODELS[0].value);
  const [apiKey, setApiKey] = useState('');

  useEffect(() => {
    if (settings?.provider === 'openrouter' && settings.model) {
      setSelectedModel(settings.model);
    }
  }, [settings]);

  const saveMutation = useMutation({
    mutationFn: (data: { provider: string; api_key: string; model?: string }) => setAiSettings(data),
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
    if (!apiKey.trim() && !settings?.has_api_key) {
      toast.error('API key cannot be empty');
      return;
    }
    saveMutation.mutate({
      provider: selectedProvider,
      api_key: apiKey.trim(),
      model: selectedProvider === 'openrouter' ? selectedModel : undefined,
    });
  };

  const connectedModelLabel =
    settings?.provider === 'openrouter' && settings.model
      ? (OPENROUTER_MODELS.find((m) => m.value === settings.model)?.label ?? settings.model)
      : null;

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
                    {connectedModelLabel && (
                      <span className="text-muted-foreground ml-2">· {connectedModelLabel}</span>
                    )}
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

              {selectedProvider === 'openrouter' && (
                <div className="space-y-2">
                  <Label htmlFor="model-select">Model</Label>
                  <select
                    id="model-select"
                    value={selectedModel}
                    onChange={(e) => setSelectedModel(e.target.value)}
                    className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {OPENROUTER_MODELS.map((m) => (
                      <option key={m.value} value={m.value}>
                        {m.label}
                      </option>
                    ))}
                  </select>
                </div>
              )}

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
