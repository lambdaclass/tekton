import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { listTasks, createTask } from '../lib/api';

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

export default function Tasks() {
  const queryClient = useQueryClient();
  const { data: tasks, isLoading } = useQuery({
    queryKey: ['tasks'],
    queryFn: listTasks,
    refetchInterval: 5000,
  });

  const [showCreate, setShowCreate] = useState(false);
  const [prompt, setPrompt] = useState('');
  const [repo, setRepo] = useState('');
  const [baseBranch, setBaseBranch] = useState('main');

  const createMutation = useMutation({
    mutationFn: createTask,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
      setShowCreate(false);
      setPrompt('');
      setRepo('');
      setBaseBranch('main');
    },
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();
    createMutation.mutate({
      prompt,
      repo,
      base_branch: baseBranch || undefined,
    });
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">Claude Tasks</h1>
        <button
          onClick={() => setShowCreate(!showCreate)}
          className="px-4 py-2 bg-purple-600 hover:bg-purple-700 rounded-lg font-medium transition-colors"
        >
          {showCreate ? 'Cancel' : 'New Task'}
        </button>
      </div>

      {showCreate && (
        <form onSubmit={handleCreate} className="mb-8 p-6 bg-gray-900 rounded-lg border border-gray-800">
          <div className="mb-4">
            <label className="block text-sm text-gray-400 mb-1">Prompt</label>
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Describe the coding task for Claude..."
              required
              rows={4}
              className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-gray-100 focus:outline-none focus:border-purple-500 resize-vertical"
            />
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-4">
            <div>
              <label className="block text-sm text-gray-400 mb-1">Repository</label>
              <input
                type="text"
                value={repo}
                onChange={(e) => setRepo(e.target.value)}
                placeholder="owner/repo"
                required
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-gray-100 focus:outline-none focus:border-purple-500"
              />
            </div>
            <div>
              <label className="block text-sm text-gray-400 mb-1">Base Branch</label>
              <input
                type="text"
                value={baseBranch}
                onChange={(e) => setBaseBranch(e.target.value)}
                placeholder="main"
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-gray-100 focus:outline-none focus:border-purple-500"
              />
            </div>
          </div>
          <button
            type="submit"
            disabled={createMutation.isPending}
            className="px-4 py-2 bg-purple-600 hover:bg-purple-700 disabled:opacity-50 rounded-lg font-medium transition-colors"
          >
            {createMutation.isPending ? 'Creating...' : 'Submit Task'}
          </button>
          {createMutation.isError && (
            <p className="mt-2 text-red-400 text-sm">{(createMutation.error as Error).message}</p>
          )}
        </form>
      )}

      {isLoading ? (
        <div className="text-gray-400">Loading tasks...</div>
      ) : !tasks?.length ? (
        <div className="text-gray-400">No tasks yet.</div>
      ) : (
        <div className="space-y-3">
          {tasks.map((t) => (
            <Link
              key={t.id}
              to={`/tasks/${t.id}`}
              className="block p-4 bg-gray-900 rounded-lg border border-gray-800 hover:border-gray-600 transition-colors"
            >
              <div className="flex items-center justify-between mb-2">
                <span className="font-mono text-sm text-gray-400">{t.id.slice(0, 8)}</span>
                <span className={`text-xs px-2 py-0.5 rounded ${STATUS_COLORS[t.status] || 'bg-gray-800 text-gray-400'}`}>
                  {t.status}
                </span>
              </div>
              <p className="text-sm line-clamp-2 mb-2">{t.prompt}</p>
              <div className="flex items-center gap-4 text-xs text-gray-500">
                <span>{t.repo}</span>
                <span>{t.base_branch}</span>
                {t.preview_url && (
                  <span className="text-green-400">{t.preview_url}</span>
                )}
                <span className="ml-auto">{new Date(t.created_at).toLocaleString()}</span>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
