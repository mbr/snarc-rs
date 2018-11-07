//! A 'snitching' atomically reference counted pointer.

pub mod narc;

use std::fmt;
use std::ops::Deref;
use std::sync::Mutex;

type Uid = usize;

#[derive(Debug, Copy, Clone)]
pub enum Site {
    SourceFile { file: &'static str, line: u32 },
    Unknown,
    // TODO: Backtrace,
}

#[derive(Debug, Clone)]
enum OriginKind {
    New,
    ClonedFrom(Uid, Box<Origin>),
    UpgradedFrom(Uid, Box<Origin>),
    DowngradedFrom(Uid, Box<Origin>),
}

#[derive(Debug, Clone)]
pub struct Origin {
    kind: OriginKind,
    site: Site,
}

#[derive(Debug)]
struct Metadata {
    strong_refs: Vec<Uid>,
    weak_refs: Vec<Uid>,
    id_counter: Uid,
}

impl Metadata {
    fn new() -> Metadata {
        Metadata {
            strong_refs: Vec::new(),
            weak_refs: Vec::new(),
            id_counter: 0,
        }
    }

    fn should_cleanup(&self) -> bool {
        self.strong_refs.is_empty() && self.weak_refs.is_empty()
    }
}
