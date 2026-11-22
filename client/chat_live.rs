use std::cell::Cell;

const POLL_MS: u32 = 1000;
const TIMEOUT_MS: u32 = 5000;
const RETRY_INITIAL_MS: u32 = 1000;
const RETRY_LIMIT_MS: u32 = 16000;
const DORMANT: u32 = u32::MAX;
const VISIBILITY_ABORT: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum Status {
    Live = 0,
    Retrying = 1,
    SignedOut = 2,
    Unavailable = 3,
}

impl Status {
    fn stopped(self) -> bool {
        matches!(self, Self::SignedOut | Self::Unavailable)
    }
}

#[derive(Clone, Copy)]
struct Poll {
    cursor: u64,
    received: u64,
    delay_ms: u32,
    retry_ms: u32,
    visible: bool,
    pending: bool,
    status: Status,
}

impl Poll {
    const fn new(cursor: u64) -> Self {
        Self {
            cursor,
            received: cursor,
            delay_ms: 0,
            retry_ms: 0,
            visible: true,
            pending: false,
            status: Status::Live,
        }
    }

    fn dormant(self) -> bool {
        self.pending || !self.visible || self.status.stopped()
    }

    fn set_visible(&mut self, visible: bool) {
        if visible && !self.visible {
            self.delay_ms = 0;
        }
        self.visible = visible;
    }

    fn begin(&mut self) -> u32 {
        if self.dormant() {
            return 0;
        }
        self.received = self.cursor;
        self.pending = true;
        TIMEOUT_MS
    }

    fn row(&mut self, id: u64) -> bool {
        if !self.pending || !self.visible || id <= self.received {
            return false;
        }
        self.received = id;
        true
    }

    fn finish(&mut self, result: u32, cursor: u64) -> bool {
        if !self.pending {
            return false;
        }
        self.pending = false;
        match result {
            401 => self.status = Status::SignedOut,
            403 | 404 => self.status = Status::Unavailable,
            VISIBILITY_ABORT => self.delay_ms = 0,
            204 => self.mark_live(),
            200 if self.visible && cursor == self.received && cursor > self.cursor => {
                self.cursor = cursor;
                self.mark_live();
                return true;
            }
            _ => {
                self.status = Status::Retrying;
                self.retry_ms = (self.retry_ms * 2).clamp(RETRY_INITIAL_MS, RETRY_LIMIT_MS);
                self.delay_ms = self.retry_ms;
            }
        }
        false
    }

    fn mark_live(&mut self) {
        self.status = Status::Live;
        self.retry_ms = 0;
        self.delay_ms = POLL_MS;
    }
}

thread_local! {
    static POLL: Cell<Poll> = const { Cell::new(Poll::new(0)) };
}

fn update<T>(change: impl FnOnce(&mut Poll) -> T) -> T {
    POLL.with(|cell| {
        let mut poll = cell.get();
        let result = change(&mut poll);
        cell.set(poll);
        result
    })
}

// The browser binding drives this clock-free controller with visibility and fetch events.
#[no_mangle]
extern "C" fn chat_init(cursor: u64) {
    POLL.with(|cell| cell.set(Poll::new(cursor)));
}

#[no_mangle]
extern "C" fn chat_set_visible(visible: u32) {
    update(|poll| poll.set_visible(visible != 0));
}

#[no_mangle]
extern "C" fn chat_delay_ms() -> u32 {
    let poll = POLL.get();
    if poll.dormant() {
        DORMANT
    } else {
        poll.delay_ms
    }
}

#[no_mangle]
extern "C" fn chat_begin() -> u32 {
    update(Poll::begin)
}

#[no_mangle]
extern "C" fn chat_cursor() -> u64 {
    POLL.get().cursor
}

#[no_mangle]
extern "C" fn chat_row(id: u64) -> u32 {
    update(|poll| u32::from(poll.row(id)))
}

#[no_mangle]
extern "C" fn chat_finish(result: u32, cursor: u64) -> u32 {
    update(|poll| u32::from(poll.finish(result, cursor)))
}

#[no_mangle]
extern "C" fn chat_status() -> u32 {
    POLL.get().status as u32
}
