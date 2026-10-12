import React, { useEffect, useState } from 'react';
import { FolderPlus, X } from 'lucide-react';

/**
 * The folders an agent connected over MCP may read files from or write files
 * to. The studio's own data, Music and Downloads always count; the user adds
 * others here (an external sample drive, a project folder).
 */

interface Folders { folders: string[]; added: string[] }

export const AgentFolders: React.FC = () => {
  const [folders, setFolders] = useState<Folders | null>(null);
  const [draft, setDraft] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void fetch('/v1/kinetic/mcp/folders')
      .then((response) => (response.ok ? response.json() : Promise.reject(new Error(`HTTP ${response.status}`))))
      .then(setFolders)
      .catch((reason: Error) => setError(reason.message));
  }, []);

  const save = async (added: string[]) => {
    setError(null);
    const response = await fetch('/v1/kinetic/mcp/folders', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ added }) });
    if (!response.ok) {
      setError(await response.text());
      return;
    }
    setFolders(await response.json());
    setDraft('');
  };

  const always = folders ? folders.folders.filter((folder) => !folders.added.includes(folder)) : [];
  return (
    <div className="space-y-2 rounded-xl border border-zinc-200 p-3 text-xs dark:border-white/10" data-testid="agent-folders">
      <div className="font-semibold text-zinc-800 dark:text-zinc-100">Folders agents may use</div>
      <p className="leading-5 text-zinc-500 dark:text-zinc-400">
        An agent can only read or write files in these folders. Before it deletes or overwrites anything, or uses the internet, the studio asks you.
      </p>
      <ul className="space-y-1 font-mono text-[11px] text-zinc-700 dark:text-zinc-200">
        {always.map((folder) => <li key={folder} className="truncate" title={folder}>{folder}</li>)}
        {folders?.added.map((folder) => (
          <li key={folder} className="flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate" title={folder}>{folder}</span>
            <button type="button" aria-label={`Remove ${folder}`} onClick={() => void save(folders.added.filter((item) => item !== folder))} className="text-zinc-400 hover:text-rose-500"><X size={12} /></button>
          </li>
        ))}
      </ul>
      <form className="flex gap-2" onSubmit={(event) => { event.preventDefault(); if (draft.trim() && folders) void save([...folders.added, draft.trim()]); }}>
        <input value={draft} onChange={(event) => setDraft(event.target.value)} placeholder="/Volumes/Samples" aria-label="Folder to add"
          className="min-w-0 flex-1 rounded-md border border-zinc-200 bg-transparent px-2 py-1 font-mono text-[11px] dark:border-white/10" />
        <button type="submit" className="inline-flex items-center gap-1 rounded-md border border-zinc-200 px-2 py-1 font-semibold text-zinc-700 dark:border-white/10 dark:text-zinc-200"><FolderPlus size={12} />Add</button>
      </form>
      {error && <p className="text-rose-600 dark:text-rose-300">{error}</p>}
    </div>
  );
};
