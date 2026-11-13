//! 別スレッドが書いて閲覧側へ流す応答の、読み終わりの扱い (バックアップのダウンロード・URL のファイルの中継)。

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, DuplexStream, ReadBuf};
use tokio_util::io::SyncIoBridge;

/// 書く側が最後まで書けずに閉じたとき、読み終わりをエラーにする。
/// 書く側は、書き終えたら `finished` に印を付けてから閉じる。
///
/// ただ閉じると、途中までの中身が正常に届いたように見えてしまう。エラーで終えると
/// 接続が切られ、ブラウザーはダウンロードの失敗として扱う。
pub(super) struct CompletedReader {
    reader: DuplexStream,
    finished: Arc<AtomicBool>,
    message: &'static str,
}

impl CompletedReader {
    /// `message` は書けなかったときのエラーの文言。
    fn new(reader: DuplexStream, finished: Arc<AtomicBool>, message: &'static str) -> Self {
        Self {
            reader,
            finished,
            message,
        }
    }

    /// 別スレッドで `write` に書かせ、その中身を読む側を返す。`buffer` は間に置くバッファーの大きさ。
    /// `write` が書き終えて返した書き手は、終わった印を付けてから閉じる。読む側は閉じたのを見てから印を見るため。
    /// 書けなかったとき (受け取る側が途中で切った場合を含む) は `on_error` に渡す。ログの出し方は使う側で違うため。
    pub(super) fn spawn_writer<W>(
        buffer: usize,
        message: &'static str,
        write: impl FnOnce(SyncIoBridge<DuplexStream>) -> std::io::Result<W> + Send + 'static,
        on_error: impl FnOnce(std::io::Error) + Send + 'static,
    ) -> Self
    where
        W: Send + 'static,
    {
        let (reader, writer) = tokio::io::duplex(buffer);
        let writer = SyncIoBridge::new(writer);
        let finished = Arc::new(AtomicBool::new(false));
        let finished_by_writer = Arc::clone(&finished);
        tokio::task::spawn_blocking(move || match write(writer) {
            Ok(writer) => {
                finished_by_writer.store(true, Ordering::Release);
                drop(writer);
            }
            Err(err) => on_error(err),
        });
        Self::new(reader, finished, message)
    }
}

impl AsyncRead for CompletedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.reader).poll_read(cx, buf);
        if matches!(poll, Poll::Ready(Ok(())))
            && buf.filled().len() == before
            && buf.remaining() > 0
            && !self.finished.load(Ordering::Acquire)
        {
            return Poll::Ready(Err(std::io::Error::other(self.message)));
        }
        poll
    }
}
