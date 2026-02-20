import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { listPreviews, createPreview, destroyPreview, updatePreview } from '../lib/api';

export default function Previews() {
  const queryClient = useQueryClient();
  const { data: previews, isLoading } = useQuery({
    queryKey: ['previews'],
    queryFn: listPreviews,
    refetchInterval: 10000,
  });

  const [showCreate, setShowCreate] = useState(false);
  const [repo, setRepo] = useState('');
  const [branch, setBranch] = useState('');
  const [slug, setSlug] = useState('');
  const [previewType, setPreviewType] = useState('node');

  const createMutation = useMutation({
    mutationFn: createPreview,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['previews'] });
      setShowCreate(false);
      setRepo('');
      setBranch('');
      setSlug('');
    },
  });

  const destroyMutation = useMutation({
    mutationFn: destroyPreview,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['previews'] }),
  });

  const updateMutation = useMutation({
    mutationFn: updatePreview,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['previews'] }),
  });

  const handleCreate = (e: React.FormEvent) => {
    e.preventDefault();
    createMutation.mutate({
      repo,
      branch,
      slug: slug || undefined,
      type: previewType,
    });
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">Previews</h1>
        <button
          onClick={() => setShowCreate(!showCreate)}
          className="px-4 py-2 bg-blue-600 hover:bg-blue-700 rounded-lg font-medium transition-colors"
        >
          {showCreate ? 'Cancel' : 'Create Preview'}
        </button>
      </div>

      {showCreate && (
        <form onSubmit={handleCreate} className="mb-8 p-6 bg-gray-900 rounded-lg border border-gray-800">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-4">
            <div>
              <label className="block text-sm text-gray-400 mb-1">Repository</label>
              <input
                type="text"
                value={repo}
                onChange={(e) => setRepo(e.target.value)}
                placeholder="owner/repo"
                required
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-gray-100 focus:outline-none focus:border-blue-500"
              />
            </div>
            <div>
              <label className="block text-sm text-gray-400 mb-1">Branch</label>
              <input
                type="text"
                value={branch}
                onChange={(e) => setBranch(e.target.value)}
                placeholder="main"
                required
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-gray-100 focus:outline-none focus:border-blue-500"
              />
            </div>
            <div>
              <label className="block text-sm text-gray-400 mb-1">Slug (optional)</label>
              <input
                type="text"
                value={slug}
                onChange={(e) => setSlug(e.target.value)}
                placeholder="auto-generated"
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-gray-100 focus:outline-none focus:border-blue-500"
              />
            </div>
            <div>
              <label className="block text-sm text-gray-400 mb-1">Type</label>
              <select
                value={previewType}
                onChange={(e) => setPreviewType(e.target.value)}
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-gray-100 focus:outline-none focus:border-blue-500"
              >
                <option value="node">Node.js</option>
                <option value="vertex">Vertex (Elixir)</option>
              </select>
            </div>
          </div>
          <button
            type="submit"
            disabled={createMutation.isPending}
            className="px-4 py-2 bg-green-600 hover:bg-green-700 disabled:opacity-50 rounded-lg font-medium transition-colors"
          >
            {createMutation.isPending ? 'Creating...' : 'Create'}
          </button>
          {createMutation.isError && (
            <p className="mt-2 text-red-400 text-sm">{(createMutation.error as Error).message}</p>
          )}
        </form>
      )}

      {isLoading ? (
        <div className="text-gray-400">Loading previews...</div>
      ) : !previews?.length ? (
        <div className="text-gray-400">No active previews.</div>
      ) : (
        <div className="space-y-3">
          {previews.map((p) => (
            <div
              key={p.slug}
              className="flex items-center justify-between p-4 bg-gray-900 rounded-lg border border-gray-800"
            >
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-3">
                  <span className="font-mono font-semibold">{p.slug}</span>
                  <span className="text-xs px-2 py-0.5 rounded bg-gray-800 text-gray-400">
                    {p.preview_type}
                  </span>
                </div>
                <div className="text-sm text-gray-400 mt-1">
                  {p.repo} / {p.branch}
                </div>
              </div>
              <div className="flex items-center gap-2 ml-4">
                <a
                  href={p.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="px-3 py-1.5 text-sm bg-gray-800 hover:bg-gray-700 rounded transition-colors"
                >
                  Open
                </a>
                <Link
                  to={`/previews/${p.slug}`}
                  className="px-3 py-1.5 text-sm bg-gray-800 hover:bg-gray-700 rounded transition-colors"
                >
                  Logs
                </Link>
                <button
                  onClick={() => updateMutation.mutate(p.slug)}
                  disabled={updateMutation.isPending}
                  className="px-3 py-1.5 text-sm bg-yellow-700 hover:bg-yellow-600 disabled:opacity-50 rounded transition-colors"
                >
                  Update
                </button>
                <button
                  onClick={() => {
                    if (confirm(`Destroy preview "${p.slug}"?`)) {
                      destroyMutation.mutate(p.slug);
                    }
                  }}
                  disabled={destroyMutation.isPending}
                  className="px-3 py-1.5 text-sm bg-red-700 hover:bg-red-600 disabled:opacity-50 rounded transition-colors"
                >
                  Destroy
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
