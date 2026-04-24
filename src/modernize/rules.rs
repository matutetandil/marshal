//! Concrete modernization rules.
//!
//! Step 3 leaves this module empty — the registry builds with zero rules and
//! is therefore behaviorally silent. Step 4 populates `register_defaults`
//! with the 12 canonical Git modernizations (checkout, reset, stash, remote
//! families), likely splitting this file into a `rules/` directory once the
//! rule count makes that worthwhile.

use crate::modernize::Registry;

pub fn register_defaults(_registry: &mut Registry) {
    // Intentionally empty — see module doc. Concrete rules land in step 4.
}
