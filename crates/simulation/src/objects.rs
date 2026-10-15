//! Stable identities of inspectable objects, shared by the client's
//! inspector and the search engine's results.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectRef {
    Galaxy,
    List {
        id: String,
    },
    /// By its system's key, a star system as a whole: its star, or a multiple
    /// system's outermost barycentre. By a member ID (`key#B`), one star of a
    /// multiple system.
    Star {
        id: String,
    },
    /// A pair within a triple or quadruple system, such as its close pair.
    Barycentre {
        id: String,
    },
    Planet {
        id: String,
    },
    /// A man-made object of any kind (`craft::CraftKind`), by its own ID.
    Craft {
        id: String,
    },
}
