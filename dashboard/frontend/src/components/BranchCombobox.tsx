import { useState, useEffect, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { listBranches } from '@/lib/api';
import { Input } from '@/components/ui/input';

interface BranchComboboxProps {
  repo: string;
  value: string;
  onChange: (branch: string) => void;
}

export default function BranchCombobox({ repo, value, onChange }: BranchComboboxProps) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState(value);
  const wrapperRef = useRef<HTMLDivElement>(null);

  // Parse owner/repo
  const parts = repo.split('/');
  const owner = parts.length === 2 ? parts[0] : '';
  const repoName = parts.length === 2 ? parts[1] : '';

  const { data: branches } = useQuery({
    queryKey: ['branches', owner, repoName],
    queryFn: () => listBranches(owner, repoName),
    enabled: !!owner && !!repoName,
    staleTime: 60_000,
  });

  // Sync external value changes
  useEffect(() => {
    setFilter(value);
  }, [value]);

  // Close dropdown on outside click
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  const filtered = (branches ?? []).filter((b) =>
    b.toLowerCase().includes(filter.toLowerCase())
  );

  return (
    <div ref={wrapperRef} className="relative">
      <Input
        value={filter}
        onChange={(e) => {
          setFilter(e.target.value);
          onChange(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        placeholder="main"
      />
      {open && filtered.length > 0 && (
        <ul className="absolute z-50 mt-1 max-h-48 w-full overflow-auto rounded-md border border-border bg-popover text-sm shadow-md">
          {filtered.slice(0, 50).map((branch) => (
            <li
              key={branch}
              className="cursor-pointer px-3 py-1.5 hover:bg-accent hover:text-accent-foreground"
              onMouseDown={(e) => {
                e.preventDefault();
                setFilter(branch);
                onChange(branch);
                setOpen(false);
              }}
            >
              {branch}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
