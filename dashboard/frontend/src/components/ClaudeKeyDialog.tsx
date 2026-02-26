import { useMutation, useQueryClient } from '@tanstack/react-query';
import { disconnectClaude } from '@/lib/api';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Check, Unplug } from 'lucide-react';

interface ClaudeKeyDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  hasClaudeKey: boolean;
}

export default function ClaudeKeyDialog({
  open,
  onOpenChange,
  hasClaudeKey,
}: ClaudeKeyDialogProps) {
  const queryClient = useQueryClient();

  const disconnectMutation = useMutation({
    mutationFn: disconnectClaude,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['me'] });
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Claude Account</DialogTitle>
          <DialogDescription>
            {hasClaudeKey
              ? 'Your Claude account is connected. Tasks will use your own credentials.'
              : 'Connect your Claude account to create tasks using your own credentials.'}
          </DialogDescription>
        </DialogHeader>

        <div className="mt-4">
          {hasClaudeKey ? (
            <div className="space-y-4">
              <div className="flex items-center gap-2 text-sm text-green-500">
                <Check className="size-4" />
                <span>Claude account connected</span>
              </div>
              <Button
                variant="outline"
                onClick={() => disconnectMutation.mutate()}
                disabled={disconnectMutation.isPending}
              >
                <Unplug className="size-4 mr-2" />
                {disconnectMutation.isPending ? 'Disconnecting...' : 'Disconnect'}
              </Button>
            </div>
          ) : (
            <Button asChild>
              <a href="/api/auth/claude/login">Connect Claude Account</a>
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
