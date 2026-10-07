//! 別スレッドが書いて閲覧側へ流す応答の、読み終わりの扱い (バックアップのダウンロード・URL のファイルの中継)。

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, DuplexStream, ReadBuf};

/// 書く側が最後まで書けずに閉じたとき、読み終わりをエラーにする。
/// 書く側は、書き終えたら `finished` に印を付けてから閉じる。
///
/// ただ閉じると、途中までの中身が正常に届いたように見えてしまう。エラーで終えると
/// 接続が切られ、ブラウザはダウンロードの失敗として扱う。
pub(super) struct CompletedReader {
    reader: DuplexStream,
    finished: Arc<AtomicBool>,
    message: &'static str,
}

impl CompletedReader {
    /// `message` は書けなかったときのエラーの文言。
    pub(super) fn new(
        reader: DuplexStream,
        finished: Arc<AtomicBool>,
        message: &'static str,
    ) -> Self {
        Self {
            reader,
            finished,
            message,
        }
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
