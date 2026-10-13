import React, { useEffect, useState } from 'react';
import { useOpenRouterSettings, useSetupStatus } from '../services/studioQueries';
import { Disc, Layers, Library, Moon, Newspaper, ScrollText, Search, SlidersHorizontal, Sun } from 'lucide-react';
import { View } from '../types';
import { useI18n } from '../context/I18nContext';
import { profileLabel } from '../services/modelCatalog';
import { ResourceMonitor } from './ResourceMonitor';
import { PlayerExtraButtons } from './player/PlayerExtraButtons';
import { STUDIO_NAME, STUDIO_WORDMARK } from '../services/brand';

interface SidebarProps {
  currentView: View;
  onNavigate: (view: View) => void;
  theme: 'light' | 'dark';
  onToggleTheme: () => void;
  user?: { username: string; isAdmin?: boolean; avatar_url?: string } | null;
  onOpenSettings?: () => void;
  isOpen?: boolean;
  onToggle?: () => void;
  /** The journal: whether it is open. */
  journalOpen?: boolean;
  onToggleJournal?: () => void;
}

type NativeSetupStatus = {
  ready: boolean;
  engine_ready: boolean;
  engine_id: string;
  selected_profile_id?: string | null;
  selected_component_ids?: string[] | null;
  /** Set when the music engine runs on another computer (Kinetic Studio). */
  engine_remote?: string | null;
};

const StatusLine: React.FC<{ active: boolean; pending?: boolean; label: string }> = ({ active, pending, label }) => (
  <div className="flex min-w-0 items-center gap-1.5 text-zinc-500 dark:text-zinc-400" title={label}>
    <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${active ? 'bg-emerald-500' : pending ? 'animate-pulse bg-amber-400' : 'bg-zinc-400 dark:bg-zinc-600'}`} />
    <span className="truncate">{label}</span>
  </div>
);

const SystemWidget: React.FC<{ isOpen?: boolean }> = ({ isOpen }) => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const setupQuery = useSetupStatus<NativeSetupStatus>();
  const setup = setupQuery.isError ? null : setupQuery.data ?? null;
  const unreachable = setupQuery.isError;
  const openRouter = useOpenRouterSettings<{ configured?: boolean }>().data ?? null;

  // The credential lives in the native server, never in browser storage, so
  // the badge asks the server whether one is configured.
  const openRouterConfigured = openRouter?.configured === true;
  const engineReady = setup?.engine_ready === true;
  const modelReady = setup?.ready === true;
  const profile = setup?.selected_profile_id
    ? profileLabel(t, { id: setup.selected_profile_id, label: setup.selected_profile_id })
    : (setup?.selected_component_ids?.length ? t('customSet') : t('noProfile'));
  // Without a model set on disk the engine is not starting, it is waiting.
  const engineLabel = unreachable ? t('engineUnavailable') : engineReady ? t('engineReachable') : modelReady ? t('engineStarting') : tt('engineWaitsForModels');
  const modelLabel = setup?.engine_remote ? `Models on ${setup.engine_remote}` : modelReady ? `${t('profileReady')}: ${profile}` : t('profileNotInstalled');

  if (!isOpen) {
    return (
      <div className="flex w-full justify-center py-2" title={`${engineLabel} · ${modelLabel}`}>
        <span className={`h-2 w-2 rounded-full ${engineReady && modelReady ? 'bg-emerald-500' : unreachable ? 'bg-rose-500' : 'animate-pulse bg-amber-400'}`} />
      </div>
    );
  }

  // With nobody answering, the model and provider lines would be guesses. Say
  // the one thing that is actually known: the service is not responding.
  if (unreachable) {
    return (
      <div className="space-y-1.5 rounded-xl border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-[10px]">
        <StatusLine active={false} label={t('serviceUnavailable')} />
      </div>
    );
  }

  return (
    <div className="space-y-1.5 rounded-xl border border-zinc-200/70 bg-zinc-50/80 px-3 py-2 text-[10px] dark:border-white/5 dark:bg-zinc-900/50">
      <StatusLine active={engineReady && modelReady} pending={!unreachable && !(engineReady && modelReady)} label={engineLabel} />
      <StatusLine active={modelReady} pending={!unreachable && !modelReady} label={modelLabel} />
      <StatusLine active={openRouterConfigured} label={openRouterConfigured ? t('openRouterConfigured') : t('openRouterNotConfigured')} />
    </div>
  );
};

export const Sidebar: React.FC<SidebarProps> = ({
  currentView,
  onNavigate,
  theme,
  onToggleTheme,
  user,
  onOpenSettings,
  isOpen = true,
  onToggle,
  journalOpen = false,
  onToggleJournal,
}) => {
  const { t } = useI18n();

  return (
    <>
      {isOpen && onToggle && (
        <div className="fixed inset-0 z-40 bg-black/60 backdrop-blur-xs md:hidden" onClick={onToggle} />
      )}

      <aside className={`fixed inset-y-0 left-0 z-50 flex h-full min-h-0 shrink-0 flex-col overflow-hidden border-r border-zinc-200 bg-white py-4 transition-[width,transform] duration-300 dark:border-white/5 dark:bg-suno-sidebar md:relative md:inset-auto ${isOpen ? 'w-[min(20rem,calc(100vw-2.5rem))] md:w-[220px]' : 'w-[72px]'}`}>
        <div className="mb-5 flex min-w-0 items-center justify-between gap-2 px-4">
          {/* The studio's wordmark, set in type: a letterspaced name, no logo art. */}
          <button type="button" className="flex min-w-0 items-center gap-3" onClick={() => (isOpen || !onToggle ? onNavigate('create') : onToggle())} title={isOpen || !onToggle ? STUDIO_NAME : t('expandSidebar')}>
            {isOpen ? (
              <span className="truncate text-[22px] font-extrabold uppercase leading-none tracking-[0.08em] text-zinc-900 dark:text-white">{STUDIO_WORDMARK}</span>
            ) : (
              <img src="/brand/yue2.png" alt="" className="h-9 w-9 rounded-[10px]" />
            )}
          </button>
          {onToggle && isOpen && (
            <button type="button" onClick={onToggle} className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-zinc-500 transition-colors hover:text-black dark:text-zinc-500 dark:hover:text-white" title={t('collapseSidebar')}>
              <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
              </svg>
            </button>
          )}
        </div>

        {/* The person, under the name, the way a music service shows its account. */}
        <div className="mb-4 px-3">
          <button type="button" onClick={onOpenSettings} className={`flex w-full items-center gap-3 rounded-xl text-zinc-600 transition-colors hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-white/5 ${isOpen ? 'px-2 py-2' : 'aspect-square justify-center'}`} title={`${user?.username ?? ''} - ${t('settings')}`}>
            <span className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-full bg-linear-to-br from-orange-400 via-pink-500 to-violet-600 text-sm font-bold text-white">{(user?.username ?? '?').charAt(0).toUpperCase()}</span>
            {isOpen && (
              <span className="min-w-0 flex-1 text-left">
                <span className="block truncate text-sm font-medium text-zinc-900 dark:text-white">{user?.username}</span>
                <span className="block truncate text-xs text-zinc-500">{t('settings')}</span>
              </span>
            )}
          </button>
        </div>

        <nav className="flex min-h-0 w-full flex-1 flex-col gap-0.5 overflow-y-auto px-3 scrollbar-hide">
          <NavItem icon={<Disc size={18} />} label={t('create')} active={currentView === 'create'} onClick={() => onNavigate('create')} isExpanded={isOpen} />
          <NavItem icon={<Library size={18} />} label={t('library')} active={currentView === 'library'} onClick={() => onNavigate('library')} isExpanded={isOpen} />
          <NavItem icon={<Search size={18} />} label={t('search')} active={currentView === 'search'} onClick={() => onNavigate('search')} isExpanded={isOpen} />
          <NavItem icon={<Layers size={18} />} label={t('adaptersNav')} active={currentView === 'adapters'} onClick={() => onNavigate('adapters')} isExpanded={isOpen} />
          <NavItem icon={<SlidersHorizontal size={18} />} label={t('studioTools')} active={currentView === 'tools'} onClick={() => onNavigate('tools')} isExpanded={isOpen} />
          <NavItem icon={<Newspaper size={18} />} label={t('news')} active={currentView === 'news'} onClick={() => onNavigate('news')} isExpanded={isOpen} />
          {onToggleJournal && <NavItem icon={<ScrollText size={18} />} label={t('journal')} active={journalOpen} opens={journalOpen} onClick={onToggleJournal} isExpanded={isOpen} />}

          <div className="mt-auto flex flex-col gap-2 pt-4">
            {/* the files panel docks its chip here */}
            <div id="dock-slot" className="flex flex-col" />
            {/* the player's extras, also when no song is loaded and the player bar is away */}
            <div className={isOpen ? 'flex flex-col gap-1' : 'flex flex-col gap-2'}>
              <PlayerExtraButtons layout={isOpen ? 'list' : 'rail'} />
            </div>
            <ResourceMonitor isOpen={isOpen} />
            <SystemWidget isOpen={isOpen} />
            <button type="button" onClick={onToggleTheme} className={`flex w-full items-center gap-3 rounded-xl text-zinc-500 transition-all duration-200 hover:bg-zinc-100 hover:text-black dark:text-zinc-400 dark:hover:bg-white/5 dark:hover:text-white ${isOpen ? 'justify-start px-3 py-2.5' : 'aspect-square justify-center'}`} title={theme === 'dark' ? t('lightMode') : t('darkMode')}>
              <span className="shrink-0">{theme === 'dark' ? <Sun size={20} /> : <Moon size={20} />}</span>
              {isOpen && <span className="truncate text-sm font-medium">{theme === 'dark' ? t('lightMode') : t('darkMode')}</span>}
            </button>
          </div>
        </nav>
      </aside>
    </>
  );
};

interface NavItemProps {
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  onClick: () => void;
  isExpanded?: boolean;
  /** For an item that opens a panel rather than a page: whether it is open. */
  opens?: boolean;
}

const NavItem: React.FC<NavItemProps> = ({ icon, label, active, onClick, isExpanded, opens }) => (
  <button type="button" onClick={onClick} aria-expanded={opens} aria-current={active ? 'page' : undefined} className={`group relative flex w-full items-center gap-3 overflow-hidden rounded-lg transition-colors duration-150 ${isExpanded ? 'justify-start px-3 py-2' : 'aspect-square justify-center'} ${active ? 'text-zinc-950 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white'}`} title={label}>
    <span className={`shrink-0 ${active ? '' : 'opacity-80 group-hover:opacity-100'}`}>{icon}</span>
    {isExpanded && <span className={`truncate text-[15px] ${active ? 'font-semibold' : 'font-medium'}`}>{label}</span>}
  </button>
);
