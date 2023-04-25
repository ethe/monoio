#![allow(missing_docs)]
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use super::Stream;

pub enum Either<A, B> {
    A(A),
    B(B),
}

pub trait Merge {
    type Stream: Stream;

    fn merge(self) -> Self::Stream;
}

pub struct MergeStream<F1, F2> {
    f1: F1,
    f2: F2,
    f1_done: bool,
    f2_done: bool,
}

impl<F1, F2> Unpin for MergeStream<F1, F2> {}

impl<'f, F1, F2> Future for &'f mut MergeStream<F1, F2>
where
    F1: Future,
    F2: Future,
{
    type Output = Option<Either<F1::Output, F2::Output>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.get_mut();
        if this.f1_done && this.f2_done {
            return Poll::Ready(None);
        }

        if !this.f1_done {
            if let Poll::Ready(o) = unsafe { Pin::new_unchecked(&mut this.f1).poll(cx) } {
                this.f1_done = true;
                return Poll::Ready(Some(Either::A(o)));
            }
        }
        if !this.f2_done {
            if let Poll::Ready(o) = unsafe { Pin::new_unchecked(&mut this.f2).poll(cx) } {
                this.f2_done = true;
                return Poll::Ready(Some(Either::B(o)));
            }
        }

        Poll::Pending
    }
}

impl<F1, F2> Stream for MergeStream<F1, F2>
where
    F1: Future,
    F2: Future,
{
    type Item = Either<F1::Output, F2::Output>;

    type NextFuture<'a> = impl 'a + Future<Output = Option<Self::Item>>
    where
        Self: 'a;

    fn next(&mut self) -> Self::NextFuture<'_> {
        self
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (2, Some(2))
    }
}

impl<F1, F2> Merge for (F1, F2)
where
    F1: Future,
    F2: Future,
{
    type Stream = MergeStream<F1, F2>;

    fn merge(self) -> Self::Stream {
        MergeStream {
            f1: self.0,
            f2: self.1,
            f1_done: false,
            f2_done: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Either, Merge};
    use crate::{io::stream::Stream, net::TcpListener, time::sleep, IoUringDriver, RuntimeBuilder};

    #[test]
    fn merge_timeout() {
        let mut runtime = RuntimeBuilder::<IoUringDriver>::new()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async move {
            let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
            loop {
                let timeout = sleep(Duration::from_millis(100));
                let mut merge = (timeout, listener.accept()).merge();
                while let Some(result) = merge.next().await {
                    match result {
                        Either::A(_) => println!("timeout!"),
                        Either::B(stream) => {
                            // because io-uring is fully asynchronous, when timeout happened socket
                            // has already accept a stream, should handle
                            // it.
                            let _ = stream;
                        }
                    }
                }
            }
        });
    }
}
