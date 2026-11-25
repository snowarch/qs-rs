pub mod clock;
pub mod colorquantizer;
pub mod common;
pub mod datastream;
pub mod desktopentry;
pub mod errorlog;
pub mod fieldregion;
pub mod folderlist;
pub mod global;
pub mod icons;
pub mod instance;
pub mod log;
pub mod pool;
pub mod proc;
pub mod spring;
pub mod util;
pub mod watch;

/// Bumped when a reload puts a new tree in place: what registers after it belongs to the new generation.
pub static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
