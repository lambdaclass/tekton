import { useState, useRef, useMemo } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Send, ImagePlus, X, Loader2 } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { listTaskMessages, sendTaskMessage, uploadImage, parseImageUrls, type TaskMessage } from '@/lib/api';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { timeAgo } from '@/lib/utils';

interface TaskChatProps {
  taskId: string;
  currentUserEmail: string;
  taskStatus?: string;
}

/** Replace fenced code blocks in Claude messages with collapsible <details> elements */
function processMessageContent(content: string, sender: string): string {
  if (sender !== 'claude') return content;
  return content.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, lang: string, code: string) => {
    const summary = lang ? `Code (${lang})` : 'Code block';
    return `<details><summary>${summary}</summary>\n\n\`\`\`${lang}\n${code}\`\`\`\n</details>`;
  });
}

export default function TaskChat({ taskId, currentUserEmail, taskStatus }: TaskChatProps) {
  const queryClient = useQueryClient();
  const [message, setMessage] = useState('');
  const [chatImages, setChatImages] = useState<File[]>([]);
  const [chatImagePreviews, setChatImagePreviews] = useState<string[]>([]);
  const [chatUploading, setChatUploading] = useState(false);
  const chatImageRef = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);

  const { data: messages } = useQuery({
    queryKey: ['task-messages', taskId],
    queryFn: () => listTaskMessages(taskId),
    refetchInterval: 3000,
  });

  const addChatImages = (files: FileList | File[]) => {
    const newFiles = Array.from(files).filter((f) => f.type.startsWith('image/'));
    if (newFiles.length === 0) return;
    setChatImages((prev) => [...prev, ...newFiles]);
    for (const file of newFiles) {
      const reader = new FileReader();
      reader.onload = (e) =>
        setChatImagePreviews((prev) => [...prev, e.target?.result as string]);
      reader.readAsDataURL(file);
    }
  };

  const removeChatImage = (index: number) => {
    setChatImages((prev) => prev.filter((_, i) => i !== index));
    setChatImagePreviews((prev) => prev.filter((_, i) => i !== index));
  };

  const handleChatDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    addChatImages(e.dataTransfer.files);
  };

  const sendMutation = useMutation({
    mutationFn: async ({ content, image_urls }: { content: string; image_urls?: string[] }) => {
      return sendTaskMessage(taskId, content, image_urls);
    },
    onMutate: async ({ content }) => {
      await queryClient.cancelQueries({ queryKey: ['task-messages', taskId] });
      const previousMessages = queryClient.getQueryData<TaskMessage[]>(['task-messages', taskId]);

      const optimisticMessage: TaskMessage = {
        id: Date.now(),
        task_id: taskId,
        sender: currentUserEmail,
        content,
        created_at: new Date().toISOString(),
        image_url: null,
      };

      queryClient.setQueryData<TaskMessage[]>(
        ['task-messages', taskId],
        (old) => [...(old ?? []), optimisticMessage],
      );

      setMessage('');
      setChatImages([]);
      setChatImagePreviews([]);

      return { previousMessages };
    },
    onError: (_err, _variables, context) => {
      if (context?.previousMessages) {
        queryClient.setQueryData(['task-messages', taskId], context.previousMessages);
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['task-messages', taskId] });
    },
  });

  const handleSend = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!message.trim() && chatImages.length === 0) return;

    let image_urls: string[] | undefined;
    if (chatImages.length > 0) {
      setChatUploading(true);
      try {
        const results = await Promise.all(chatImages.map((f) => uploadImage(f)));
        image_urls = results.map((r) => r.url);
      } catch {
        setChatUploading(false);
        return;
      }
      setChatUploading(false);
    }

    sendMutation.mutate({ content: message.trim(), image_urls });
  };

  const isTyping = taskStatus === 'running_claude';

  const processedMessages = useMemo(() => {
    return messages?.map((msg) => ({
      ...msg,
      processedContent: processMessageContent(msg.content, msg.sender),
    }));
  }, [messages]);

  return (
    <div
      className={`flex flex-col h-full ${dragOver ? 'ring-2 ring-primary/50' : ''}`}
      onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
      onDragLeave={() => setDragOver(false)}
      onDrop={handleChatDrop}
    >
      {/* Messages */}
      <div className="flex-1 overflow-y-auto p-4 space-y-6">
        {!processedMessages?.length && !isTyping && (
          <p className="text-sm text-muted-foreground text-center py-4">
            No messages yet. Send a follow-up to Claude.
          </p>
        )}
        {processedMessages?.map((msg) =>
          msg.sender === 'system' ? (
            <div key={msg.id} className="flex items-center gap-3 py-1 text-xs text-muted-foreground">
              <div className="flex-1 border-t border-border/60" />
              <span className="flex items-center gap-1.5 italic shrink-0">
                {msg.content.endsWith('...') && processedMessages && msg === processedMessages[processedMessages.length - 1] && (
                  <Loader2 className="size-3 animate-spin" />
                )}
                {msg.content}
              </span>
              <div className="flex-1 border-t border-border/60" />
            </div>
          ) : (
            <article
              key={msg.id}
              className={`w-full rounded-md border-l-2 pl-4 py-3 pr-3 ${
                msg.sender === 'claude'
                  ? 'border-l-amber-500 bg-amber-500/5'
                  : 'border-l-blue-500 bg-blue-500/5'
              }`}
            >
              <div className="flex items-center gap-2 mb-1.5">
                <span
                  className={`font-semibold text-xs ${
                    msg.sender === 'claude'
                      ? 'text-amber-700 dark:text-amber-400'
                      : 'text-blue-700 dark:text-blue-400'
                  }`}
                >
                  {msg.sender === 'claude' ? 'Claude' : msg.sender === currentUserEmail ? 'You' : msg.sender}
                </span>
                <span className="text-[11px] text-muted-foreground/70" title={new Date(msg.created_at).toLocaleString()}>
                  {timeAgo(msg.created_at)}
                </span>
              </div>
              <div className="prose prose-sm dark:prose-invert max-w-none break-words [&_pre]:bg-secondary [&_pre]:rounded-md [&_pre]:p-3 [&_pre]:overflow-x-auto [&_code]:text-[0.8em] [&_:not(pre)>code]:bg-secondary [&_:not(pre)>code]:px-1 [&_:not(pre)>code]:py-px [&_:not(pre)>code]:rounded-sm [&_:not(pre)>code]:text-foreground/80 [&_:not(pre)>code]:before:content-none [&_:not(pre)>code]:after:content-none [&_p]:my-1.5 [&_ul]:my-1.5 [&_ol]:my-1.5 [&_li]:my-0.5 [&_h1]:text-base [&_h2]:text-sm [&_h3]:text-sm [&_a]:text-blue-700 dark:[&_a]:text-blue-400 [&_table]:text-xs [&_blockquote]:border-muted-foreground/30 [&_details]:border [&_details]:border-border [&_details]:rounded-md [&_details]:p-2 [&_details]:my-2 [&_summary]:cursor-pointer [&_summary]:text-xs [&_summary]:text-muted-foreground [&_summary]:font-medium">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.processedContent}</ReactMarkdown>
              </div>
              {parseImageUrls(msg.image_url).length > 0 && (
                <div className="flex flex-wrap gap-2 mt-2">
                  {parseImageUrls(msg.image_url).map((url, i) => (
                    <a key={i} href={url} target="_blank" rel="noopener noreferrer">
                      <img
                        src={url}
                        alt={`Attached image ${i + 1}`}
                        className="max-h-48 rounded-md border border-border hover:opacity-90 transition-opacity"
                      />
                    </a>
                  ))}
                </div>
              )}
            </article>
          )
        )}
        {isTyping && (
          <div className="flex items-center gap-1 px-4 py-3 bg-muted rounded-lg w-fit">
            <span className="text-xs text-muted-foreground mr-2">Claude is working</span>
            <span className="typing-dot" style={{animationDelay: '0s'}} />
            <span className="typing-dot" style={{animationDelay: '0.15s'}} />
            <span className="typing-dot" style={{animationDelay: '0.3s'}} />
          </div>
        )}
      </div>

      {/* Input area */}
      <div className="border-t border-border p-4">
        {chatImagePreviews.length > 0 && (
          <div className="mb-2 flex flex-wrap gap-2">
            {chatImagePreviews.map((preview, i) => (
              <div key={i} className="relative inline-block">
                <img
                  src={preview}
                  alt={`Attachment preview ${i + 1}`}
                  className="max-h-24 rounded-md border border-border"
                />
                <button
                  type="button"
                  onClick={() => removeChatImage(i)}
                  className="absolute -top-2 -right-2 bg-destructive text-destructive-foreground rounded-full p-0.5"
                >
                  <X className="size-3" />
                </button>
              </div>
            ))}
          </div>
        )}
        <form onSubmit={handleSend} className="flex gap-2">
          <input
            ref={chatImageRef}
            type="file"
            accept="image/png,image/jpeg,image/gif,image/webp"
            multiple
            className="hidden"
            onChange={(e) => {
              if (e.target.files?.length) addChatImages(e.target.files);
              e.target.value = '';
            }}
          />
          <Button
            type="button"
            size="icon"
            variant="outline"
            onClick={() => chatImageRef.current?.click()}
            disabled={sendMutation.isPending || chatUploading}
          >
            <ImagePlus className="size-4" />
          </Button>
          <Input
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            placeholder="Send a follow-up message..."
            disabled={sendMutation.isPending || chatUploading}
            className="flex-1"
          />
          <Button type="submit" size="icon" disabled={sendMutation.isPending || chatUploading || (!message.trim() && chatImages.length === 0)}>
            <Send className="size-4" />
          </Button>
        </form>
      </div>
    </div>
  );
}
