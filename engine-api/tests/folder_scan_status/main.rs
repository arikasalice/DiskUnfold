//! Folder rows and browse contexts carry the per-folder scan state of the pinned version.
//!
//! The producer invariants of `FolderFlags` and `excludedChildren` are fixed here through
//! the open, position and continuation paths of the GUI browse session.
mod support;

mod collector;
mod excluded;
mod rows;
mod sample;
