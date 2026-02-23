import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Send, CheckCircle, ExternalLink } from 'lucide-react';
import { listTaskMessages, sendTaskMessage } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

interface TaskChatProps {
  taskId: string;
  currentUserEmail: string;
  previewUrl?: string;
}

export default function TaskChat({ taskId, currentUserEmail, previewUrl }: TaskChatProps) {
  const queryClient = useQueryClient();
  const [message, setMessage] = useState('');

  const { data: messages } = useQuery({
    queryKey: ['task-messages', taskId],
    queryFn: () => listTaskMessages(taskId),
    refetchInterval: 3000,
  });

  const sendMutation = useMutation({
    mutationFn: (content: string) => sendTaskMessage(taskId, content),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['task-messages', taskId] });
      setMessage('');
    },
  });

  const handleSend = (e: React.FormEvent) => {
    e.preventDefault();
    if (!message.trim()) return;
    sendMutation.mutate(message.trim());
  };

  const handleMarkDone = () => {
    sendMutation.mutate('__done__');
  };

  const senderColor = (sender: string) => {
    if (sender === currentUserEmail) return 'bg-primary/10 border-primary/20';
    if (sender === 'claude') return 'bg-muted border-border';
    return 'bg-secondary/30 border-secondary';
  };

  return (
    <Card className="mb-6">
      <CardHeader className="py-3">
        <CardTitle className="text-base flex items-center justify-between">
          <span>Follow-up Chat</span>
          <Button
            variant="outline"
            size="sm"
            onClick={handleMarkDone}
            disabled={sendMutation.isPending}
            className="text-green-400 border-green-500/30 hover:bg-green-500/10"
          >
            <CheckCircle className="size-4 mr-1" />
            Mark Done
          </Button>
        </CardTitle>
      </CardHeader>
      <CardContent className="pt-0">
        {previewUrl && (
          <div className="mb-4 p-3 rounded-lg bg-blue-500/10 border border-blue-500/20 flex items-center gap-2 text-sm">
            <ExternalLink className="size-4 text-blue-400 shrink-0" />
            <span className="text-muted-foreground">Check the current result:</span>
            <a
              href={previewUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="text-blue-400 hover:text-blue-300 font-medium truncate"
            >
              {previewUrl}
            </a>
          </div>
        )}
        <div className="space-y-3 mb-4 max-h-80 overflow-y-auto pr-1">
          {!messages?.length && (
            <p className="text-sm text-muted-foreground text-center py-4">
              {previewUrl
                ? 'Check the preview above, then send a follow-up if something needs fixing.'
                : 'No messages yet. Send a follow-up to Claude.'}
            </p>
          )}
          {messages?.map((msg) => (
            <div
              key={msg.id}
              className={`rounded-lg border p-3 text-sm ${senderColor(msg.sender)}`}
            >
              <div className="flex items-center justify-between mb-1">
                <span className="font-medium text-xs text-muted-foreground">
                  {msg.sender === currentUserEmail ? 'You' : msg.sender}
                </span>
                <span className="text-xs text-muted-foreground">
                  {new Date(msg.created_at).toLocaleTimeString()}
                </span>
              </div>
              <p className="whitespace-pre-wrap">{msg.content}</p>
            </div>
          ))}
        </div>
        <form onSubmit={handleSend} className="flex gap-2">
          <Input
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            placeholder="Send a follow-up message..."
            disabled={sendMutation.isPending}
            className="flex-1"
          />
          <Button type="submit" size="icon" disabled={sendMutation.isPending || !message.trim()}>
            <Send className="size-4" />
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
