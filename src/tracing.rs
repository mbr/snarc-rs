//! Tracing utilities.
//!
//! Data types to track origin and history across call sites.

use std::fmt;

/// Unique ID type to identify ancestors.
pub type Uid = usize;

/// Call site.
#[derive(Debug, Clone)]
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
    Annotated(String),
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Site::SourceFile { file, line } => write!(f, "{}:{}", file, line),
            Site::Unknown => write!(f, "?"),
            Site::Annotated(ref s) => write!(f, "\"{}\"", s),
        }
    }
}

/// Reference origin.
#[derive(Debug, Clone)]
pub enum OriginKind {
    /// New object Instantiation (resulting ID),
    New(Uid),
    // FIXME: IDs need to be for current, not passed down.
    // FIXME: Move ID into Origin.
    /// Cloned from another reference, (original ID, site of original reference).
    Cloned(Uid, Box<Origin>),
    /// Upgraded from a weak reference, (weak reference ID, site of weak reference).
    Upgraded(Uid, Box<Origin>),
    /// Downgraded from a strong reference, (strong reference ID, site of strong reference).
    Downgraded(Uid, Box<Origin>),
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

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut cur = Some(self);

        while let Some(link) = cur {
            match link.kind {
                OriginKind::New(id) => {
                    write!(f, "new<{}>[{}]", id, link.site)?;
                    cur = None;
                }
                OriginKind::Cloned(id, ref parent) => {
                    write!(f, "clone<{}>[{}]", id, link.site)?;
                    cur = Some(parent);
                }
                OriginKind::Upgraded(id, ref parent) => {
                    write!(f, "upgrade<{}>[{}]", id, link.site)?;
                    cur = Some(parent);
                }
                OriginKind::Downgraded(id, ref parent) => {
                    write!(f, "downgrade<{}>[{}]", id, link.site)?;
                    cur = Some(parent);
                }
            };

            if cur.is_some() {
                write!(f, " <- ")?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Origin, OriginKind, Site};

    #[test]
    fn format_origin_single() {
        let subj = Origin {
            kind: OriginKind::New(15),
            site: Site::Unknown,
        };

        assert_eq!("new<15>[?]".to_string(), format!("{}", subj));

        let subj = Origin {
            kind: OriginKind::New(123),
            site: Site::SourceFile {
                file: "foo.rs",
                line: 543,
            },
        };

        assert_eq!("new<123>[foo.rs:543]".to_string(), format!("{}", subj));

        let subj = Origin {
            kind: OriginKind::New(0),
            site: Site::Annotated("dummy".to_string()),
        };

        assert_eq!("new<0>[\"dummy\"]".to_string(), format!("{}", subj));
    }

    #[test]
    fn format_origin_chain() {
        let one = Origin {
            kind: OriginKind::New(0),
            site: Site::SourceFile {
                file: "orig.rs",
                line: 999,
            },
        };

        let two = Origin {
            kind: OriginKind::Cloned(1, Box::new(one)),
            site: Site::Annotated("step two".to_string()),
        };

        let three = Origin {
            kind: OriginKind::Downgraded(2, Box::new(two)),
            site: Site::Unknown,
        };

        let four = Origin {
            kind: OriginKind::Upgraded(3, Box::new(three)),
            site: Site::SourceFile {
                file: "final.rs",
                line: 42,
            },
        };

        assert_eq!(
            "upgrade<3>[final.rs:42] <- downgrade<2>[?] \
             <- clone<1>[\"step two\"] <- new<0>[orig.rs:999]",
            format!("{}", four)
        );
    }
}
