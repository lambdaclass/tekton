import { useState, useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { getSshKey, setSshKey, deleteSshKey } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { KeyRound } from 'lucide-react';

export default function Settings() {
  const queryClient = useQueryClient();
  const { data, isLoading } = useQuery({
    queryKey: ['ssh-key'],
    queryFn: getSshKey,
  });

  const [keyValue, setKeyValue] = useState('');
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    if (data?.ssh_public_key != null) {
      setKeyValue(data.ssh_public_key);
    }
  }, [data]);

  const saveMutation = useMutation({
    mutationFn: (key: string) => setSshKey(key),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['ssh-key'] });
      setDirty(false);
    },
  });

  const removeMutation = useMutation({
    mutationFn: () => deleteSshKey(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['ssh-key'] });
      setKeyValue('');
      setDirty(false);
    },
  });

  const handleSave = (e: React.FormEvent) => {
    e.preventDefault();
    saveMutation.mutate(keyValue.trim());
  };

  return (
    <div className="space-y-8">
      <h1 className="text-2xl font-bold">Settings</h1>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <KeyRound className="size-5" />
            SSH Public Key
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground mb-4">
            Add your SSH public key to access preview containers via SSH.
            Once set, you can connect with the SSH command shown on each preview card.
          </p>

          {isLoading ? (
            <p className="text-muted-foreground text-sm">Loading...</p>
          ) : (
            <form onSubmit={handleSave} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="ssh-key">Public Key</Label>
                <textarea
                  id="ssh-key"
                  className="flex w-full rounded-md border border-input bg-background px-3 py-2 text-sm font-mono ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 min-h-[100px] resize-y"
                  placeholder="ssh-ed25519 AAAA... user@host"
                  value={keyValue}
                  onChange={(e) => {
                    setKeyValue(e.target.value);
                    setDirty(true);
                  }}
                />
              </div>

              <div className="flex gap-2">
                <Button type="submit" disabled={saveMutation.isPending || !dirty}>
                  {saveMutation.isPending ? 'Saving...' : 'Save Key'}
                </Button>
                {data?.ssh_public_key && (
                  <Button
                    type="button"
                    variant="destructive"
                    onClick={() => removeMutation.mutate()}
                    disabled={removeMutation.isPending}
                  >
                    {removeMutation.isPending ? 'Removing...' : 'Remove Key'}
                  </Button>
                )}
              </div>

              {saveMutation.isError && (
                <p className="text-destructive text-sm">
                  {(saveMutation.error as Error).message}
                </p>
              )}
              {saveMutation.isSuccess && (
                <p className="text-green-600 text-sm">SSH key saved.</p>
              )}
            </form>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
