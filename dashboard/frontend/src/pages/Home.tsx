import { Link } from 'react-router-dom';

export default function Home() {
  return (
    <div>
      <h1 className="text-2xl font-bold mb-6">Preview Dashboard</h1>
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <Link
          to="/previews"
          className="block p-6 bg-gray-900 rounded-lg border border-gray-800 hover:border-gray-600 transition-colors"
        >
          <h2 className="text-lg font-semibold mb-2">Previews</h2>
          <p className="text-gray-400">
            Manage preview containers. Create, destroy, and update previews from any branch.
          </p>
        </Link>
        <Link
          to="/tasks"
          className="block p-6 bg-gray-900 rounded-lg border border-gray-800 hover:border-gray-600 transition-colors"
        >
          <h2 className="text-lg font-semibold mb-2">Claude Tasks</h2>
          <p className="text-gray-400">
            Submit coding tasks to Claude. Monitor progress and view live output.
          </p>
        </Link>
      </div>
    </div>
  );
}
