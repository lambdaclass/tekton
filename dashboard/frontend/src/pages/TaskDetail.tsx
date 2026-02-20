import { useEffect, useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import LogViewer from '../components/LogViewer';
import { getTask, connectTaskOutput } from '../lib/api';

const STATUS_COLORS: Record<string, string> = {
  pending: 'bg-gray-800 text-gray-400',
  creating_agent: 'bg-blue-900 text-blue-300',
  cloning: 'bg-blue-900 text-blue-300',
  running_claude: 'bg-purple-900 text-purple-300',
  pushing: 'bg-blue-900 text-blue-300',
  creating_preview: 'bg-blue-900 text-blue-300',
  completed: 'bg-green-900 text-green-300',
  failed: 'bg-red-900 text-red-300',
};

export default function TaskDetail() {
  const { id } = useParams<{ id: string }>();
  const [ws, setWs] = useState<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);

  const { data: task } = useQuery({
    queryKey: ['task', id],
    queryFn: () => getTask(id!),
    enabled: !!id,
    refetchInterval: 3000,
  });

  useEffect(() => {
    if (!id) return;

    const socket = connectTaskOutput(id);
    socket.addEventListener('open', () => setConnected(true));
    socket.addEventListener('close', () => setConnected(false));
    setWs(socket);

    return () => {
      socket.close();
    };
  }, [id]);

  return (
    <div>
      <div className="flex items-center gap-4 mb-6">
        <Link to="/tasks" className="text-gray-400 hover:text-gray-100 transition-colors">
          &larr; Tasks
        </Link>
        <h1 className="text-2xl font-bold font-mono">{id?.slice(0, 8)}</h1>
        {task && (
          <span className={`text-xs px-2 py-0.5 rounded ${STATUS_COLORS[task.status] || 'bg-gray-800 text-gray-400'}`}>
            {task.status}
          </span>
        )}
        <span
          className={`text-xs px-2 py-0.5 rounded ${
            connected ? 'bg-green-900 text-green-300' : 'bg-gray-800 text-gray-400'
          }`}
        >
          {connected ? 'Live' : 'Disconnected'}
        </span>
      </div>

      {task && (
        <div className="mb-6 p-4 bg-gray-900 rounded-lg border border-gray-800">
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
            <div>
              <span className="text-gray-400">Repo:</span>
              <span className="ml-2">{task.repo}</span>
            </div>
            <div>
              <span className="text-gray-400">Base:</span>
              <span className="ml-2">{task.base_branch}</span>
            </div>
            {task.branch_name && (
              <div>
                <span className="text-gray-400">Branch:</span>
                <span className="ml-2 font-mono">{task.branch_name}</span>
              </div>
            )}
            {task.preview_url && (
              <div>
                <span className="text-gray-400">Preview:</span>
                <a
                  href={task.preview_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="ml-2 text-blue-400 hover:text-blue-300"
                >
                  {task.preview_slug}
                </a>
              </div>
            )}
          </div>
          <div className="mt-3 pt-3 border-t border-gray-800">
            <span className="text-gray-400 text-sm">Prompt:</span>
            <p className="mt-1 text-sm whitespace-pre-wrap">{task.prompt}</p>
          </div>
          {task.error_message && (
            <div className="mt-3 pt-3 border-t border-gray-800">
              <span className="text-red-400 text-sm">Error:</span>
              <p className="mt-1 text-sm text-red-300">{task.error_message}</p>
            </div>
          )}
        </div>
      )}

      <LogViewer ws={ws} />
    </div>
  );
}
