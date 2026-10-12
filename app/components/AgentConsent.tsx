import React, { useEffect, useState } from 'react';
import { Globe, ShieldAlert, Trash2 } from 'lucide-react';
import { useBridgeCommand } from '../services/mcpBridge';

/**
 * An agent connected over MCP asks before it deletes or overwrites anything,
 * or reaches the internet: the studio shows what it wants to do and runs it
 * only on Allow. A request nobody answers is refused after two minutes, the
 * time the service waits.
 */

interface Request {
  tool: string;
  title: string;
  description: string;
  arguments: string;
  deletes: boolean;
  internet: boolean;
  answer: (allowed: boolean) => void;
}

const WAIT_MS = 120_000;

export const AgentConsent: React.FC = () => {
  const [queue, setQueue] = useState<Request[]>([]);

  useBridgeCommand('agent_consent', (args) => new Promise((resolve) => {
    let settled = false;
    const request: Request = {
      tool: String(args.tool ?? ''),
      title: String(args.title ?? args.tool ?? ''),
      description: String(args.description ?? ''),
      arguments: String(args.arguments ?? ''),
      deletes: Boolean(args.deletes),
      internet: Boolean(args.internet),
      answer: (allowed) => {
        if (settled) return;
        settled = true;
        window.clearTimeout(timer);
        setQueue((current) => current.filter((item) => item !== request));
        resolve({ allowed });
      },
    };
    const timer = window.setTimeout(() => request.answer(false), WAIT_MS);
    setQueue((current) => [...current, request]);
  }));

  const shown = queue[0];

  useEffect(() => {
    if (!shown) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') shown.answer(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [shown]);

  if (!shown) return null;
  return (
    <div className="fixed inset-0 z-[200] grid place-items-center bg-black/50 p-4" role="dialog" aria-modal="true" aria-labelledby="agent-consent-title" data-testid="agent-consent">
      <div className="w-full max-w-md rounded-2xl border border-zinc-200 bg-white p-5 shadow-2xl dark:border-white/10 dark:bg-zinc-900">
        <div className="flex items-center gap-2 text-amber-600 dark:text-amber-300">
          <ShieldAlert size={18} />
          <h2 id="agent-consent-title" className="text-sm font-bold">An agent asks to use &ldquo;{shown.title}&rdquo;</h2>
        </div>
        <p className="mt-2 text-xs leading-5 text-zinc-600 dark:text-zinc-300">{shown.description}</p>
        <div className="mt-3 flex flex-wrap gap-2 text-[11px] font-semibold">
          {shown.deletes && <span className="inline-flex items-center gap-1 rounded-md bg-rose-500/10 px-2 py-0.5 text-rose-600 dark:text-rose-300"><Trash2 size={11} />Deletes or overwrites</span>}
          {shown.internet && <span className="inline-flex items-center gap-1 rounded-md bg-sky-500/10 px-2 py-0.5 text-sky-700 dark:text-sky-300"><Globe size={11} />Uses the internet (may download or cost money)</span>}
        </div>
        {shown.arguments && shown.arguments !== '{}' && (
          <pre className="mt-3 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-zinc-200 bg-zinc-50 p-2 font-mono text-[11px] text-zinc-700 dark:border-white/10 dark:bg-black/30 dark:text-zinc-200">{shown.arguments}</pre>
        )}
        <p className="mt-2 font-mono text-[10px] text-zinc-400">{shown.tool}{queue.length > 1 ? ` · ${queue.length - 1} more waiting` : ''}</p>
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" onClick={() => shown.answer(false)} className="rounded-lg border border-zinc-200 px-3 py-1.5 text-xs font-semibold text-zinc-700 dark:border-white/10 dark:text-zinc-200" autoFocus>Deny</button>
          <button type="button" onClick={() => shown.answer(true)} className="rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-3 py-1.5 text-xs font-semibold text-white">Allow</button>
        </div>
      </div>
    </div>
  );
};
