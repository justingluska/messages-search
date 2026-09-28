//! Event names (backend → UI). The status shapes themselves live in
//! ms-engine (`AppStatus`) and ms-core (`IndexProgress`), mirrored by
//! src/lib/types.ts.

pub const EVENT_INDEX_PROGRESS: &str = "index-progress";
pub const EVENT_INDEX_CHANGED: &str = "index-changed";
