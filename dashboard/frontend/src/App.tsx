import { Routes, Route, Navigate } from 'react-router-dom';
import Layout from './components/Layout';
import Home from './pages/Home';
import Previews from './pages/Previews';
import PreviewDetail from './pages/PreviewDetail';
import Tasks from './pages/Tasks';
import TaskDetail from './pages/TaskDetail';
import Admin from './pages/Admin';
import Settings from './pages/Settings';
import Webhooks from './pages/Webhooks';
import CostDashboard from './pages/CostDashboard';
import AuditLog from './pages/AuditLog';
import IntakeBoard from './pages/IntakeBoard';

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
        <Route path="/cost" element={<CostDashboard />} />
        <Route path="/audit" element={<AuditLog />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="/intake" element={<IntakeBoard />} />
        <Route path="/webhooks" element={<Webhooks />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
