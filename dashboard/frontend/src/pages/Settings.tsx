import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getAiSettings,
  setAiSettings,
  deleteAiSettings,
  getGlobalAiSettings,
  setGlobalAiSettings,
  deleteGlobalAiSettings,
  getMe,
} from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { BrainCircuit, Building2, Info } from 'lucide-react';
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
  { value: 'anthropic/claude-sonnet-4.6', label: 'Claude Sonnet 4.6 (recommended)', free: false },
  { value: 'anthropic/claude-opus-4.6', label: 'Claude Opus 4.6', free: false },
  { value: 'openai/gpt-4o', label: 'GPT-4o', free: false },
  { value: 'openai/o3', label: 'o3', free: false },
  { value: 'google/gemini-2.5-flash', label: 'Gemini 2.5 Flash', free: false },
  { value: 'google/gemini-2.5-pro', label: 'Gemini 2.5 Pro', free: false },
  { value: 'meta-llama/llama-3.3-70b-instruct', label: 'Llama 3.3 70B', free: false },
  { value: 'deepseek/deepseek-r1', label: 'DeepSeek R1', free: false },
  { value: 'moonshotai/kimi-k2', label: 'Kimi K2', free: false },
  { value: 'nvidia/nemotron-3-nano-30b-a3b:free', label: 'Nvidia Nemotron 3 Nano 30B (free)', free: true },
  { value: 'arcee-ai/trinity-large-preview:free', label: 'Arcee Trinity Large (free)', free: true },
];

export default function Settings() {
  const queryClient = useQueryClient();
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });
  const isAdmin = me?.role === 'admin';

  const { data: settings, isLoading } = useQuery({
    queryKey: ['ai-settings'],
    queryFn: getAiSettings,
  });

  const [selectedProvider, setSelectedProvider] = useState<ProviderValue>('anthropic');
  const [userSelectedModel, setUserSelectedModel] = useState<string | null>(null);
  const selectedModel = userSelectedModel
    ?? (settings?.provider === 'openrouter' && settings?.model ? settings.model : null)
    ?? OPENROUTER_MODELS[0].value;
  const [apiKey, setApiKey] = useState('');

  const saveMutation = useMutation({
    mutationFn: (data: { provider: string; api_key?: string; model?: string }) => setAiSettings(data),
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

  // ── Global AI settings (admin only) ──
  const { data: globalSettings, isLoading: globalLoading } = useQuery({
    queryKey: ['global-ai-settings'],
    queryFn: getGlobalAiSettings,
    enabled: isAdmin,
  });

  const [globalProvider, setGlobalProvider] = useState<ProviderValue>('anthropic');
  const [globalUserSelectedModel, setGlobalUserSelectedModel] = useState<string | null>(null);
  const globalSelectedModel = globalUserSelectedModel
    ?? (globalSettings?.provider === 'openrouter' && globalSettings?.model ? globalSettings.model : null)
    ?? OPENROUTER_MODELS[0].value;
  const [globalApiKey, setGlobalApiKey] = useState('');

  const globalSaveMutation = useMutation({
    mutationFn: (data: { provider: string; api_key?: string; model?: string }) =>
      setGlobalAiSettings(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['global-ai-settings'] });
      queryClient.invalidateQueries({ queryKey: ['ai-settings'] });
      setGlobalApiKey('');
      toast.success('Global AI provider saved');
    },
    onError: (e: Error) => toast.error(e.message),
  });

  const globalDeleteMutation = useMutation({
    mutationFn: deleteGlobalAiSettings,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['global-ai-settings'] });
      queryClient.invalidateQueries({ queryKey: ['ai-settings'] });
      toast.success('Global AI provider disconnected');
    },
    onError: (e: Error) => toast.error(e.message),
  });

  const globalCurrentProvider = PROVIDERS.find((p) => p.value === globalProvider)!;

  const handleGlobalSave = () => {
    if (!globalApiKey.trim() && !globalSettings?.has_api_key) {
      toast.error('API key cannot be empty');
      return;
    }
    const trimmed = globalApiKey.trim();
    globalSaveMutation.mutate({
      provider: globalProvider,
      ...(trimmed ? { api_key: trimmed } : {}),
      model: globalProvider === 'openrouter' ? globalSelectedModel : undefined,
    });
  };

  const globalConnectedModelLabel =
    globalSettings?.provider === 'openrouter' && globalSettings.model
      ? (OPENROUTER_MODELS.find((m) => m.value === globalSettings.model)?.label ??
          globalSettings.model)
      : null;

  const currentProvider = PROVIDERS.find((p) => p.value === selectedProvider)!;

  const handleSave = () => {
    if (!apiKey.trim() && !settings?.has_api_key) {
      toast.error('API key cannot be empty');
      return;
    }
    const trimmed = apiKey.trim();
    saveMutation.mutate({
      provider: selectedProvider,
      ...(trimmed ? { api_key: trimmed } : {}),
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
                    onChange={(e) => setUserSelectedModel(e.target.value)}
                    className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {OPENROUTER_MODELS.map((m) => (
                      <option key={m.value} value={m.value}>
                        {m.label}
                      </option>
                    ))}
                  </select>
                  {OPENROUTER_MODELS.find((m) => m.value === selectedModel)?.free && (
                    <div className="rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3">
                      <p className="text-sm font-semibold text-destructive">⚠ Free models will not work reliably</p>
                      <p className="mt-1 text-xs text-destructive/80">
                        Free tier models lack the tool use capabilities required by Claude Code. Tasks will likely
                        fail or produce no output. Add credits to your OpenRouter account to use a capable model.
                      </p>
                    </div>
                  )}
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

              {!settings?.has_api_key && settings?.has_global_fallback && (
                <div className="flex items-start gap-2 rounded-md border border-blue-500/30 bg-blue-500/10 px-4 py-3">
                  <Info className="size-4 mt-0.5 shrink-0 text-blue-600 dark:text-blue-400" />
                  <p className="text-sm text-blue-700 dark:text-blue-300">
                    No personal key configured — the organization's global API key will be used as a
                    fallback.
                  </p>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>

      {isAdmin && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Building2 className="size-5" />
              Organization AI Provider (Fallback)
            </CardTitle>
            <p className="text-sm text-muted-foreground">
              This key is used as a fallback for users who haven't configured their own API key.
            </p>
          </CardHeader>
          <CardContent className="space-y-6">
            {globalLoading ? (
              <p className="text-muted-foreground text-sm">Loading...</p>
            ) : (
              <>
                {globalSettings?.has_api_key && (
                  <div className="flex items-center justify-between rounded-md border px-4 py-3">
                    <div className="text-sm">
                      <span className="text-muted-foreground">Connected provider: </span>
                      <span className="font-medium capitalize">
                        {PROVIDERS.find((p) => p.value === globalSettings.provider)?.label ??
                          globalSettings.provider}
                      </span>
                      {globalConnectedModelLabel && (
                        <span className="text-muted-foreground ml-2">
                          · {globalConnectedModelLabel}
                        </span>
                      )}
                      <span className="text-muted-foreground ml-2">(API key stored)</span>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => globalDeleteMutation.mutate()}
                      disabled={globalDeleteMutation.isPending}
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
                          name="global-provider"
                          value={p.value}
                          checked={globalProvider === p.value}
                          onChange={() => setGlobalProvider(p.value)}
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

                {globalProvider === 'openrouter' && (
                  <div className="space-y-2">
                    <Label htmlFor="global-model-select">Model</Label>
                    <select
                      id="global-model-select"
                      value={globalSelectedModel}
                      onChange={(e) => setGlobalUserSelectedModel(e.target.value)}
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
                  <Label htmlFor="global-api-key">
                    API Key{globalSettings?.has_api_key && ' (leave blank to keep existing)'}
                  </Label>
                  <Input
                    id="global-api-key"
                    type="password"
                    placeholder={
                      globalSettings?.has_api_key
                        ? '••••••••••••••••'
                        : globalCurrentProvider.linkLabel
                    }
                    value={globalApiKey}
                    onChange={(e) => setGlobalApiKey(e.target.value)}
                    autoComplete="off"
                  />
                </div>

                <Button
                  onClick={handleGlobalSave}
                  disabled={
                    globalSaveMutation.isPending ||
                    (!globalApiKey.trim() && !globalSettings?.has_api_key)
                  }
                >
                  {globalSaveMutation.isPending
                    ? 'Saving...'
                    : globalSettings?.has_api_key
                      ? 'Update'
                      : 'Save'}
                </Button>
              </>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
