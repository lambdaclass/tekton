import { Routes, Route } from 'react-router-dom';
import Layout from './components/Layout';
import Home from './pages/Home';
import Previews from './pages/Previews';
import PreviewDetail from './pages/PreviewDetail';
import Tasks from './pages/Tasks';
import TaskDetail from './pages/TaskDetail';
import Admin from './pages/Admin';
import Settings from './pages/Settings';

export default function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route path="/" element={<Home />} />
        <Route path="/previews" element={<Previews />} />
        <Route path="/previews/:slug" element={<PreviewDetail />} />
        <Route path="/tasks" element={<Tasks />} />
        <Route path="/tasks/:id" element={<TaskDetail />} />
        <Route path="/admin" element={<Admin />} />
        <Route path="/settings" element={<Settings />} />
      </Route>
    </Routes>
  );
}
