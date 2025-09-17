use std::{
    convert::Infallible,
    pin::{Pin, pin},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake},
};

use flume::r#async::RecvFut;
use futures_util::{StreamExt, ready, stream::FuturesUnordered};
use multimap::MultiMap;
use slab::Slab;

#[derive(Default)]
struct State {
    events: Slab<Event>,
    blocks: MultiMap<usize, usize>,
    blocked_by: MultiMap<usize, usize>,
    causes: MultiMap<usize, usize>,
    caused_by: MultiMap<usize, usize>,
}

impl State {
    fn completed(&self, event: usize) -> bool {
        self.events[event].completed()
    }

    fn fire(&mut self, event: usize) {
        self.events[event].end.take().unwrap();
    }

    fn blocked(&self, event: usize) -> bool {
        !self
            .blocked_by
            .get_vec(&event)
            .unwrap_or(&Vec::new())
            .iter()
            .all(|blocked_by| self.completed(*blocked_by))
    }

    fn caused(&self, event: usize) -> bool {
        self.caused_by
            .get_vec(&event)
            .unwrap_or(&Vec::new())
            .iter()
            .any(|caused_by| self.completed(*caused_by))
    }

    fn finalize(&self) {
        for (id, event) in &self.events {
            if !self.caused(id) {
                continue;
            }
            if self.blocked(id) {
                panic!("caused but blocked");
            }
            if !event.future && !event.completed() {
                panic!("caused but never fired");
            }
        }
    }
}

#[derive(Clone)]
struct StateRef(Arc<Mutex<State>>);

struct Event {
    end: Option<flume::Sender<Infallible>>,
    wait: flume::Receiver<Infallible>,
    future: bool,
}

impl Event {
    fn completed(&self) -> bool {
        self.end.is_none()
    }
}

pub struct EventRef {
    id: usize,
    state: StateRef,
}

impl EventRef {
    pub fn events(&self) -> Events {
        Events {
            state: self.state.clone(),
        }
    }

    pub fn root(self) {
        let mut state = self.state.0.lock().unwrap();
        assert!(!state.blocked_by.contains_key(&self.id));
        assert!(!state.caused_by.contains_key(&self.id));
        state.fire(self.id);
    }

    pub fn fire(self) {
        let mut state = self.state.0.lock().unwrap();
        assert!(!state.blocked(self.id));
        assert!(state.caused(self.id));
        state.fire(self.id);
    }

    pub fn blocks(&self, blocked: &Self) {
        self.state
            .0
            .lock()
            .unwrap()
            .blocks
            .insert(self.id, blocked.id);
        self.state
            .0
            .lock()
            .unwrap()
            .blocked_by
            .insert(blocked.id, self.id);
    }

    pub fn causes(&self, caused: &Self) {
        self.state
            .0
            .lock()
            .unwrap()
            .causes
            .insert(self.id, caused.id);
        self.state
            .0
            .lock()
            .unwrap()
            .caused_by
            .insert(caused.id, self.id);
    }

    pub fn next_event(&self) -> EventRef {
        let next = self.events().event();
        self.blocks(&next);
        self.causes(&next);
        next
    }

    pub fn next_future(&self) -> EventFuture {
        self.next_event().into_future()
    }
}

pub struct EventFuture {
    event: Option<EventRef>,
    blocked_by: FuturesUnordered<RecvFut<'static, Infallible>>,
    caused_by: FuturesUnordered<RecvFut<'static, Infallible>>,
}

impl EventFuture {
    pub fn next_event(&self) -> EventRef {
        self.event.as_ref().unwrap().next_event()
    }

    pub fn next_future(&self) -> EventFuture {
        self.event.as_ref().unwrap().next_future()
    }
}

impl Future for EventFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("poll blocked_by");
        while let Some(item) = ready!(self.blocked_by.poll_next_unpin(cx)) {
            let Err(_) = item;
        }
        println!("poll caused_by");
        let Some(item) = ready!(self.caused_by.poll_next_unpin(cx)) else {
            println!("no causes");
            return Poll::Pending;
        };
        let Err(_) = item;
        self.event.take().unwrap().fire();
        Poll::Ready(())
    }
}

impl IntoFuture for EventRef {
    type Output = ();

    type IntoFuture = EventFuture;

    fn into_future(self) -> Self::IntoFuture {
        let mut state = self.state.0.lock().unwrap();
        let blocked_by = state
            .blocked_by
            .get_vec(&self.id)
            .unwrap_or(&Vec::new())
            .iter()
            .map(|blocked_by| state.events[*blocked_by].wait.clone().into_recv_async())
            .collect();
        let caused_by: FuturesUnordered<RecvFut<'static, Infallible>> = state
            .caused_by
            .get_vec(&self.id)
            .unwrap_or(&Vec::new())
            .iter()
            .map(|caused_by| state.events[*caused_by].wait.clone().into_recv_async())
            .collect();
        assert!(!caused_by.is_empty());
        state.events[self.id].future = true;
        drop(state);
        EventFuture {
            event: Some(self),
            blocked_by,
            caused_by,
        }
    }
}

#[derive(Clone)]
pub struct Events {
    state: StateRef,
}

impl Events {
    pub fn event(&self) -> EventRef {
        let (end, wait) = flume::bounded(0);
        let id = self.state.0.lock().unwrap().events.insert(Event {
            end: Some(end),
            wait,
            future: false,
        });
        EventRef {
            id,
            state: self.state.clone(),
        }
    }

    pub fn future(&self) -> EventFuture {
        self.event().into_future()
    }
}

#[derive(Default)]
struct SimpleWaker(AtomicBool);

impl Wake for SimpleWaker {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}

fn force_complete(f: impl Future<Output = ()>) {
    let mut f = pin!(f);
    loop {
        let w = Arc::new(SimpleWaker::default());
        {
            let waker = w.clone().into();
            let mut cx = Context::from_waker(&waker);
            match f.as_mut().poll(&mut cx) {
                Poll::Ready(()) => break,
                Poll::Pending => {
                    assert!(w.0.load(Ordering::Acquire));
                }
            }
        }
    }
}

pub fn run(f: impl AsyncFnOnce(Events)) {
    let state = StateRef(Arc::new(Mutex::new(State::default())));
    let events = Events { state };
    force_complete(f(events.clone()));
    events.state.0.lock().unwrap().finalize();
}

#[test]
fn stuff() {
    run(async |events| {
        use futures_util::FutureExt;
        let root = events.event();
        let a_produced = root.next_future();
        let a_received = a_produced.next_event();
        let b_produced = a_received.next_future();
        let b_received = b_produced.next_event();
        root.root();
        let mut stream = futures_util::stream::iter([
            a_produced.map(|_| "a").boxed(),
            b_produced.map(|_| "b").boxed(),
        ])
        .then(|x| x);
        assert_eq!(stream.next().await.unwrap(), "a");
        a_received.fire();
        assert_eq!(stream.next().await.unwrap(), "b");
        b_received.fire();
    });
}
