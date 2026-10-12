import React, { useEffect, useState } from 'react';
import { AlertTriangle, CheckCircle2, Loader2, Server, X } from 'lucide-react';

type Status = { saved: string | null; active: string | null; restart_needed: boolean; warning: string | null };
type Probe = { ok: boolean; url: string; error?: string; props?: Record<string, unknown> | null; warning?: string | null };

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-900 outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-black/25 dark:text-white';
const BUTTON =
  'inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-3 py-1.5 text-xs font-semibold text-zinc-700 transition hover:border-pink-300 disabled:opacity-50 dark:border-white/10 dark:text-zinc-200';

/** A few things the engine says it has loaded, when it says them. */
function describe(props: Record<string, unknown> | null | undefined): string | null {
  if (!props) return null;
  const parts = ['model', 'model_path', 'backend', 'device', 'max_batch']
    .map(key => (typeof props[key] === 'string' || typeof props[key] === 'number' ? `${key}: ${String(props[key])}` : null))
    .filter(Boolean);
  return parts.length ? parts.join(' · ') : null;
}

/**
 * Kinetic Studio's choice of where the music engine runs: this computer, or
 * yue-server on another machine (an NVIDIA box) reached through an SSH tunnel.
 * The address is read at launch, so a change applies after a restart.
 */
export const RemoteEngineSettings: React.FC<{ compact?: boolean }> = ({ compact = false }) => {
  const [status, setStatus] = useState<Status | null>(null);
  const [open, setOpen] = useState(!compact);
  const [url, setUrl] = useState('');
  const [busy, setBusy] = useState<null | 'test' | 'save'>(null);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void fetch('/v1/kinetic/engine')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error(`HTTP ${response.status}`))))
      .then((body: Status) => {
        setStatus(body);
        setUrl(body.saved ?? '');
        if (body.saved || body.active) setOpen(true);
      })
      .catch(reason => setError(`The engine setting could not be read: ${reason instanceof Error ? reason.message : String(reason)}`));
  }, []);

  const test = async () => {
    setBusy('test'); setError(null); setProbe(null);
    try {
      const response = await fetch('/v1/kinetic/engine/test', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ url }) });
      const body = await response.json().catch(() => null);
      if (!response.ok) setError(body?.error ?? `HTTP ${response.status}`);
      else setProbe(body as Probe);
    } finally {
      setBusy(null);
    }
  };

  const save = async (next: string | null) => {
    setBusy('save'); setError(null);
    try {
      const response = await fetch('/v1/kinetic/engine', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ url: next }) });
      const body = await response.json().catch(() => null);
      if (!response.ok) { setError(body?.error ?? `HTTP ${response.status}`); return; }
      setStatus(body as Status);
      setUrl((body as Status).saved ?? '');
    } finally {
      setBusy(null);
    }
  };

  if (compact && !open) {
    return (
      <button type="button" onClick={() => setOpen(true)} className="mt-4 inline-flex items-center gap-1.5 text-xs font-semibold text-pink-600 hover:text-pink-500 dark:text-pink-300">
        <Server size={13} />Use a music engine on another computer instead
      </button>
    );
  }

  return (
    <section className="mt-5 rounded-xl border border-zinc-200 p-4 dark:border-white/10">
      <div className="flex items-center justify-between gap-2">
        <h3 className="flex items-center gap-1.5 text-sm font-bold text-zinc-900 dark:text-white"><Server size={15} />Music engine on another computer</h3>
        {compact && <button type="button" onClick={() => setOpen(false)} className="rounded p-1 text-zinc-400 hover:text-zinc-700" title="Close"><X size={14} /></button>}
      </div>
      <p className="mt-1.5 text-xs leading-5 text-zinc-500 dark:text-zinc-400">
        Kinetic Studio can send its work to yue-server running on a computer with an NVIDIA card instead of running the model on this Mac.
        That computer needs yue-server and its own model files; nothing is downloaded or installed here, and this Mac's engine is not started.
        LoRA adapters and the model set are whatever that engine was started with.
      </p>
      <div className="mt-3 rounded-lg bg-zinc-50 p-3 text-[11px] leading-5 text-zinc-600 dark:bg-black/25 dark:text-zinc-300">
        <p className="font-semibold">Recommended: an SSH tunnel</p>
        <p>On the GPU computer, start yue-server listening on its own loopback only (<code>--host 127.0.0.1 --port 18087</code>). On this Mac, keep this running in Terminal:</p>
        <pre className="mt-1 overflow-x-auto rounded bg-white/70 p-2 font-mono dark:bg-black/30">ssh -N -L 18187:127.0.0.1:18087 you@gpu-computer</pre>
        <p>then use the address <code>http://127.0.0.1:18187</code>. yue-server has no password and does not encrypt, so do not expose it to a network directly.</p>
      </div>

      <label className="mt-3 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500">Engine address</label>
      <input className={`${CONTROL} mt-1`} value={url} placeholder="http://127.0.0.1:18187" onChange={event => { setUrl(event.target.value); setProbe(null); }} />
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <button type="button" className={BUTTON} disabled={!url.trim() || busy !== null} onClick={() => void test()}>
          {busy === 'test' && <Loader2 size={12} className="animate-spin" />}Test connection
        </button>
        <button type="button" className={BUTTON} disabled={!url.trim() || busy !== null || url.trim() === status?.saved} onClick={() => void save(url)}>Use this engine</button>
        {status?.saved && <button type="button" className={BUTTON} disabled={busy !== null} onClick={() => void save(null)}>Use this Mac's engine</button>}
      </div>

      {probe && (
        <p className={`mt-2 flex gap-1.5 text-xs leading-5 ${probe.ok ? 'text-emerald-700 dark:text-emerald-300' : 'text-red-600 dark:text-red-400'}`}>
          {probe.ok ? <CheckCircle2 size={14} className="mt-0.5 shrink-0" /> : <X size={14} className="mt-0.5 shrink-0" />}
          {probe.ok ? `yue-server answered at ${probe.url}.${describe(probe.props) ? ` ${describe(probe.props)}` : ''}` : `${probe.url}: ${probe.error}`}
        </p>
      )}
      {(probe?.warning || status?.warning) && (
        <p className="mt-2 flex gap-1.5 text-xs leading-5 text-amber-700 dark:text-amber-300"><AlertTriangle size={14} className="mt-0.5 shrink-0" />{probe?.warning ?? status?.warning}</p>
      )}
      {status && (
        <p className="mt-2 text-xs leading-5 text-zinc-500">
          In use now: {status.active ? <code>{status.active}</code> : "this Mac's own engine"}.
          {status.restart_needed && <strong className="ml-1 text-pink-600 dark:text-pink-300">Quit and reopen Kinetic Studio to switch to {status.saved ? status.saved : "this Mac's engine"}.</strong>}
        </p>
      )}
      {error && <p className="mt-2 text-xs text-red-600 dark:text-red-400">{error}</p>}
    </section>
  );
};
