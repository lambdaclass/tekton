import { Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { Container, BrainCircuit } from 'lucide-react';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { listPreviews, listTasks } from '@/lib/api';

export default function Home() {
  const { data: previews } = useQuery({
    queryKey: ['previews'],
    queryFn: listPreviews,
  });
  const { data: tasks } = useQuery({
    queryKey: ['tasks'],
    queryFn: listTasks,
  });

  return (
    <div>
      <h1 className="text-2xl font-bold mb-6">Preview Dashboard</h1>
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <Link to="/previews">
          <Card className="hover:border-muted-foreground/25 transition-colors">
            <CardHeader>
              <div className="flex items-center gap-3">
                <Container className="size-5 text-muted-foreground" />
                <CardTitle>Previews</CardTitle>
                {previews && (
                  <span className="ml-auto text-sm tabular-nums text-muted-foreground">
                    {previews.length}
                  </span>
                )}
              </div>
              <CardDescription>
                Manage preview containers. Create, destroy, and update previews from any branch.
              </CardDescription>
            </CardHeader>
          </Card>
        </Link>
        <Link to="/tasks">
          <Card className="hover:border-muted-foreground/25 transition-colors">
            <CardHeader>
              <div className="flex items-center gap-3">
                <BrainCircuit className="size-5 text-muted-foreground" />
                <CardTitle>Claude Tasks</CardTitle>
                {tasks && (
                  <span className="ml-auto text-sm tabular-nums text-muted-foreground">
                    {tasks.length}
                  </span>
                )}
              </div>
              <CardDescription>
                Submit coding tasks to Claude. Monitor progress and view live output.
              </CardDescription>
            </CardHeader>
          </Card>
        </Link>
      </div>
    </div>
  );
}
