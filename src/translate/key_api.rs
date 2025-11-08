use regex::Regex;

pub struct KeyApiRegex {
    pub thread_spawn: Regex,
    pub thread_join: Regex,
    pub scope_spwan: Regex,
    pub scope_join: Regex,
    pub condvar_notify: Regex,
    pub condvar_wait: Regex,

    pub channel_send: Regex,
    pub channel_recv: Regex,

    pub atomic_load: Regex,
    pub atomic_store: Regex,
}

impl KeyApiRegex {
    pub fn new() -> Self {
        Self {
            thread_spawn: Regex::new(r"std::thread[:a-zA-Z0-9_#\{\}]*::spawn").unwrap(),
            thread_join: Regex::new(r"std::thread[:a-zA-Z0-9_#\{\}]*::join").unwrap(),
            scope_spwan: Regex::new(r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::spawn").unwrap(),
            scope_join: Regex::new(r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::join").unwrap(),
            condvar_notify: Regex::new(r"condvar[:a-zA-Z0-9_#\{\}]*::notify").unwrap(),
            condvar_wait: Regex::new(r"condvar[:a-zA-Z0-9_#\{\}]*::wait").unwrap(),
            channel_send: Regex::new(r"mpsc[:a-zA-Z0-9_#\{\}]*::send").unwrap(),
            channel_recv: Regex::new(r"mpsc[:a-zA-Z0-9_#\{\}]*::recv").unwrap(),
            atomic_load: Regex::new(r"atomic[:a-zA-Z0-9]*::load").unwrap(),
            atomic_store: Regex::new(r"atomic[:a-zA-Z0-9]*::store").unwrap(),
        }
    }
}
