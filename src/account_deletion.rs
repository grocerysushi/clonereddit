use std::path::Path;

use crate::{auth::AuthStore, model::State};

pub fn finish_pending(state: &mut State, path: &Path, auth: &mut AuthStore) -> Result<(), String> {
    let handles: Vec<_> = auth
        .pending_account_deletions()
        .map(str::to_owned)
        .collect();
    if handles.is_empty() {
        return Ok(());
    }

    // Keep the durable cleanup record until the site snapshot is saved without the profile.
    for handle in &handles {
        state.delete_account_profile(handle);
    }
    state
        .save(path)
        .map_err(|error| format!("Could not save account deletion cleanup. {error}"))?;
    auth.finish_account_deletions(&handles)
}
