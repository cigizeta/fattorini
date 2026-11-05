//! The pipeline contract between `smithy` and `arena`: the files each writes for the
//! other, their paths, and the helpers both use. Each format has one definition here, writer and
//! reader together with their round-trip tests, so the two binaries cannot drift apart.

pub mod data_root;
pub mod generation;
pub mod lan;
pub mod players;
pub mod preprocessing_config;
pub mod protocol;
pub mod spsa;
pub mod remote;
pub mod repository_root;
pub mod segments;
pub mod system_memory;
