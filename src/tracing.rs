//! A 'snitching' atomically reference counted pointer.

use std::fmt;
use std::ops::Deref;
use std::sync::Mutex;

pub type Uid = usize;

#[derive(Debug, Copy, Clone)]
pub enum Site {
    SourceFile { file: &'static str, line: u32 },
    Unknown,
    // TODO: Backtrace,
}

#[derive(Debug, Clone)]
pub enum OriginKind {
    New,
    ClonedFrom(Uid, Box<Origin>),
    UpgradedFrom(Uid, Box<Origin>),
    DowngradedFrom(Uid, Box<Origin>),
}

#[derive(Debug, Clone)]
pub struct Origin {
    pub kind: OriginKind,
    pub site: Site,
}
