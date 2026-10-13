/**
 * Strings of Kinetic Studio's music-service layout: the queue beside the page
 * and the actions that fill it.
 */

const en = {
  queue: 'Queue',
  queueNowPlaying: 'Now playing',
  queueUpNext: 'Next in queue',
  queueEmpty: 'Nothing is queued after this song. Add songs from their menu.',
  queueClear: 'Clear queue',
  queueClose: 'Close queue',
  queueRemove: 'Remove from queue',
  queueAdd: 'Add to queue',
  queueAdded: 'Added to the queue',
};

type Strings = Record<keyof typeof en, string>;

const zh: Strings = {
  queue: '播放队列',
  queueNowPlaying: '正在播放',
  queueUpNext: '接下来播放',
  queueEmpty: '这首歌之后没有排队的歌曲。可以从歌曲菜单添加。',
  queueClear: '清空队列',
  queueClose: '关闭队列',
  queueRemove: '从队列中移除',
  queueAdd: '加入队列',
  queueAdded: '已加入队列',
};

const ja: Strings = {
  queue: 'キュー',
  queueNowPlaying: '再生中',
  queueUpNext: '次に再生',
  queueEmpty: 'この曲の後に予約された曲はありません。曲のメニューから追加できます。',
  queueClear: 'キューをクリア',
  queueClose: 'キューを閉じる',
  queueRemove: 'キューから削除',
  queueAdd: 'キューに追加',
  queueAdded: 'キューに追加しました',
};

const ko: Strings = {
  queue: '재생 대기열',
  queueNowPlaying: '재생 중',
  queueUpNext: '다음 곡',
  queueEmpty: '이 곡 다음에 대기 중인 곡이 없습니다. 곡 메뉴에서 추가하세요.',
  queueClear: '대기열 비우기',
  queueClose: '대기열 닫기',
  queueRemove: '대기열에서 제거',
  queueAdd: '대기열에 추가',
  queueAdded: '대기열에 추가했습니다',
};

const ru: Strings = {
  queue: 'Очередь',
  queueNowPlaying: 'Сейчас играет',
  queueUpNext: 'Дальше в очереди',
  queueEmpty: 'После этой песни в очереди ничего нет. Добавьте песни из их меню.',
  queueClear: 'Очистить очередь',
  queueClose: 'Закрыть очередь',
  queueRemove: 'Убрать из очереди',
  queueAdd: 'Добавить в очередь',
  queueAdded: 'Добавлено в очередь',
};

export const kineticLayoutStrings = { en, zh, ja, ko, ru };
