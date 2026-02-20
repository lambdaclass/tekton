import { Link, Outlet, useNavigate } from 'react-router-dom';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { getMe, logout } from '../lib/api';

export default function Layout() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: user, isLoading, error } = useQuery({
    queryKey: ['me'],
    queryFn: getMe,
    retry: false,
  });

  if (isLoading) {
    return (
      <div className="min-h-screen bg-gray-950 text-gray-100 flex items-center justify-center">
        <div className="text-gray-400">Loading...</div>
      </div>
    );
  }

  if (error || !user) {
    return (
      <div className="min-h-screen bg-gray-950 text-gray-100 flex items-center justify-center">
        <div className="text-center">
          <h1 className="text-2xl font-bold mb-4">Preview Dashboard</h1>
          <p className="text-gray-400 mb-6">Sign in with your @lambdaclass.com Google account</p>
          <a
            href="/api/auth/login"
            className="inline-block px-6 py-3 bg-blue-600 hover:bg-blue-700 rounded-lg font-medium transition-colors"
          >
            Sign in with Google
          </a>
        </div>
      </div>
    );
  }

  const handleLogout = async () => {
    await logout();
    queryClient.clear();
    navigate('/');
    window.location.reload();
  };

  return (
    <div className="min-h-screen bg-gray-950 text-gray-100">
      <nav className="border-b border-gray-800 bg-gray-900">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex items-center justify-between h-14">
            <div className="flex items-center gap-6">
              <Link to="/" className="font-bold text-lg">Dashboard</Link>
              <Link to="/previews" className="text-gray-400 hover:text-gray-100 transition-colors">
                Previews
              </Link>
              <Link to="/tasks" className="text-gray-400 hover:text-gray-100 transition-colors">
                Tasks
              </Link>
            </div>
            <div className="flex items-center gap-4">
              <span className="text-sm text-gray-400">{user.email}</span>
              <button
                onClick={handleLogout}
                className="text-sm text-gray-400 hover:text-gray-100 transition-colors"
              >
                Logout
              </button>
            </div>
          </div>
        </div>
      </nav>
      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <Outlet />
      </main>
    </div>
  );
}
