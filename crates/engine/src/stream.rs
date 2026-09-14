use futures::channel::mpsc::{self, Sender};
use futures::stream::{self, Stream, StreamExt};
use std::future::Future;

pub fn channel<T, F, Fut>(size: usize, f: F) -> impl Stream<Item = T>
where
    F: FnOnce(Sender<T>) -> Fut,
    Fut: Future<Output = ()>,
{
    let (sender, receiver) = mpsc::channel(size);

    let task = stream::once(f(sender)).filter_map(|()| std::future::ready(None));

    stream::select(receiver, task)
}
