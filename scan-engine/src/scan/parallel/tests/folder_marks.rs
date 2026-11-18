//! Per-folder excluded-child counts and unread marks reported by the
//! collectors to the result build.
use super::*;
use du_analysis_index::ResultVersion;
use du_core_contracts::result::{ResultItem, ResultItemKind};
use std::collections::{HashMap, HashSet};
mod cancel;
mod excluded;
mod fixture;
mod pause;
