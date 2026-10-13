import React from 'react';
import { ListMusic, Pause, Play, X } from 'lucide-react';
import { Song } from '../types';
import { useI18n } from '../context/I18nContext';
import { SongCover } from './AlbumCover';
import { captionSummary } from '../services/examples';
import { named } from '../services/accessibleName';

interface QueuePanelProps {
  queue: Song[];
  queueIndex: number;
  currentSong: Song | null;
  isPlaying: boolean;
  onPlay: (song: Song) => void;
  onRemove: (index: number) => void;
  onClear: () => void;
  onClose: () => void;
}

/** The play queue beside the page: the song playing, then what follows it. */
export const QueuePanel: React.FC<QueuePanelProps> = ({ queue, queueIndex, currentSong, isPlaying, onPlay, onRemove, onClear, onClose }) => {
  const { t } = useI18n();
  const upNext = queueIndex >= 0 ? queue.slice(queueIndex + 1).map((song, offset) => ({ song, index: queueIndex + 1 + offset })) : [];

  return (
    <aside className="flex h-full min-h-0 w-[300px] shrink-0 flex-col border-l border-zinc-200 bg-white dark:border-white/5 dark:bg-suno">
      <header className="flex h-14 shrink-0 items-center gap-2 px-4">
        <ListMusic size={18} className="text-zinc-500" />
        <h2 className="flex-1 text-base font-semibold">{t('queue')}</h2>
        {upNext.length > 0 && (
          <button type="button" onClick={onClear} className="rounded-md px-2 py-1 text-xs text-zinc-500 transition-colors hover:text-zinc-900 dark:hover:text-white">
            {t('queueClear')}
          </button>
        )}
        <button type="button" onClick={onClose} {...named(t('queueClose'))} className="rounded-full p-1.5 text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-zinc-900 dark:hover:bg-white/10 dark:hover:text-white">
          <X size={16} />
        </button>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-4 custom-scrollbar">
        {currentSong && (
          <section className="mb-4">
            <h3 className="px-2 pb-2 text-sm text-zinc-500">{t('queueNowPlaying')}</h3>
            <QueueRow song={currentSong} current isPlaying={isPlaying} onPlay={() => onPlay(currentSong)} />
          </section>
        )}
        <section>
          <h3 className="px-2 pb-2 text-sm text-zinc-500">{t('queueUpNext')}</h3>
          {upNext.length === 0 ? (
            <p className="px-2 text-xs leading-5 text-zinc-500">{t('queueEmpty')}</p>
          ) : (
            upNext.map(({ song, index }) => (
              <QueueRow key={`${song.id}-${index}`} song={song} onPlay={() => onPlay(song)} onRemove={() => onRemove(index)} />
            ))
          )}
        </section>
      </div>
    </aside>
  );
};

const QueueRow: React.FC<{ song: Song; current?: boolean; isPlaying?: boolean; onPlay: () => void; onRemove?: () => void }> = ({ song, current, isPlaying, onPlay, onRemove }) => {
  const { t } = useI18n();
  return (
    <div className="group flex items-center gap-3 rounded-lg p-2 transition-colors hover:bg-zinc-100 dark:hover:bg-white/[0.05]">
      <button type="button" onClick={onPlay} {...named(current && isPlaying ? t('pause') : t('play'))} className="relative h-12 w-12 shrink-0 overflow-hidden rounded-md bg-zinc-200 dark:bg-zinc-800">
        <SongCover song={song} size="full" className="h-full w-full" />
        <span className={`absolute inset-0 flex items-center justify-center bg-black/40 text-white transition-opacity ${current ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}`}>
          {current && isPlaying ? <Pause size={16} fill="currentColor" /> : <Play size={16} fill="currentColor" className="ml-0.5" />}
        </span>
      </button>
      <div className="min-w-0 flex-1">
        <p className={`truncate text-sm font-medium ${current ? 'text-pink-600 dark:text-pink-400' : 'text-zinc-900 dark:text-white'}`}>{song.title || t('untitled')}</p>
        <p className="truncate text-xs text-zinc-500">{captionSummary(song.style || '')}</p>
      </div>
      {onRemove && (
        <button type="button" onClick={onRemove} {...named(t('queueRemove'))} className="rounded-full p-1 text-zinc-400 opacity-0 transition-opacity hover:text-zinc-900 group-hover:opacity-100 focus-visible:opacity-100 dark:hover:text-white">
          <X size={14} />
        </button>
      )}
    </div>
  );
};
