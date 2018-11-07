//! Tracing utilities.
//!
//! Data types to track origin and history across call sites.

/// Unique ID type to identify ancestors.
pub type Uid = usize;

/// Call site.
#[derive(Debug, Copy, Clone)]
pub enum Site {
    /// File/line location inside a source file.
    SourceFile {
        /// Source file.
        file: &'static str,
        /// Line number, starting at 1.
        line: u32,
    },
    /// Unknown call site.
    ///
    /// Used, when no information about the original call site was available at runtime.
    Unknown,
    // TODO: Backtrace,
    // TODO: Annotation,
}

/// Reference origin.
#[derive(Debug, Clone)]
pub enum OriginKind {
    /// New object Instantiation.
    New,
    /// Cloned from another reference, (original ID, site of original reference).
    ClonedFrom(Uid, Box<Origin>),
    /// Upgraded from a weak reference, (weak reference ID, site of weak reference).
    UpgradedFrom(Uid, Box<Origin>),
    /// Downgraded from a strong reference, (strong reference ID, site of strong reference).
    DowngradedFrom(Uid, Box<Origin>),
}

/// Describes origin and location of a new reference creation.
#[derive(Debug, Clone)]
pub struct Origin {
    /// The kind of reference creation (new, via clone, downgrade, ...). In case there is a parent
    /// instance, its origin information will be contained in the `OriginKind` instance.
    pub kind: OriginKind,
    /// The site where the new instation occured.
    pub site: Site,
}
