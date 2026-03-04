import { useState, useRef } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Send, CheckCircle, ExternalLink, ImagePlus, X, Loader2 } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { listTaskMessages, sendTaskMessage, uploadImage, parseImageUrls } from '@/lib/api';
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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['task-messages', taskId] });
      setMessage('');
      setChatImages([]);
      setChatImagePreviews([]);
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

  const handleMarkDone = () => {
    sendMutation.mutate({ content: '__done__', image_urls: undefined });
  };

  const senderColor = (sender: string) => {
    if (sender === currentUserEmail) return 'bg-primary/10 border-primary/20';
    if (sender === 'claude') return 'bg-muted border-border';
    return 'bg-secondary/30 border-secondary';
  };

  return (
    <div
      className={`flex h-full flex-col ${dragOver ? 'ring-2 ring-primary/50 rounded-lg' : ''}`}
      onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
      onDragLeave={() => setDragOver(false)}
      onDrop={handleChatDrop}
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border px-4 py-2">
        <span className="text-sm font-medium">Follow-up Chat</span>
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
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto p-4 space-y-3">
        {previewUrl && (
          <div className="p-3 rounded-lg bg-blue-500/10 border border-blue-500/20 flex items-center gap-2 text-sm">
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
        {!messages?.length && (
          <p className="text-sm text-muted-foreground text-center py-4">
            {previewUrl
              ? 'Check the preview above, then send a follow-up if something needs fixing.'
              : 'No messages yet. Send a follow-up to Claude.'}
          </p>
        )}
        {messages?.map((msg, idx) =>
          msg.sender === 'system' ? (
            <div key={msg.id} className="flex items-center justify-center gap-1.5 text-xs text-muted-foreground py-1">
              {msg.content.endsWith('...') && messages && idx === messages.length - 1 && (
                <Loader2 className="size-3 animate-spin" />
              )}
              {msg.content}
            </div>
          ) : (
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
              <div className="prose prose-sm prose-invert max-w-none break-words [&_pre]:bg-black/30 [&_pre]:rounded-md [&_pre]:p-3 [&_pre]:overflow-x-auto [&_code]:text-[0.8em] [&_:not(pre)>code]:bg-white/8 [&_:not(pre)>code]:px-1 [&_:not(pre)>code]:py-px [&_:not(pre)>code]:rounded-sm [&_:not(pre)>code]:text-orange-300/90 [&_:not(pre)>code]:before:content-none [&_:not(pre)>code]:after:content-none [&_p]:my-1.5 [&_ul]:my-1.5 [&_ol]:my-1.5 [&_li]:my-0.5 [&_h1]:text-base [&_h2]:text-sm [&_h3]:text-sm [&_a]:text-blue-400 [&_table]:text-xs [&_blockquote]:border-muted-foreground/30">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content}</ReactMarkdown>
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
            </div>
          )
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
