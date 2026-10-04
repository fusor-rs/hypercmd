#[derive(Clone, Copy, PartialEq)]
pub(super) struct ProjectFile {
    pub(super) path: &'static str,
    pub(super) bytes: u64,
}

include!(concat!(env!("OUT_DIR"), "/files.rs"));
