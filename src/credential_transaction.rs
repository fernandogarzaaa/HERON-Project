//! Fail-closed credential replacement. Backend errors never cross this boundary:
//! OS errors may contain the credential, account name, or provider details.
use anyhow::{bail, Result};

pub(crate) trait CredentialStore {
    fn read(&mut self) -> Result<Option<String>>;
    fn write(&mut self, value: &str) -> Result<()>;
    fn delete(&mut self) -> Result<()>;
}

pub(crate) const UNCERTAIN: &str = "Credential state is uncertain: the OS vault could not confirm rollback. Revoke this connector key manually in Keychain or Credential Manager before retrying.";

fn write_verified(store: &mut impl CredentialStore, value: &str) -> Result<()> {
    store.write(value)?;
    if store.read()?.as_deref() != Some(value) {
        bail!("Credential verification failed.");
    }
    Ok(())
}

pub(crate) fn delete_verified(store: &mut impl CredentialStore) -> Result<()> {
    // A backend acknowledging deletion does not prove the credential is gone.
    if store.delete().is_err() || !matches!(store.read(), Ok(None)) {
        bail!("Could not verify connector credential revocation. Revoke the key manually in the OS credential manager before retrying.");
    }
    Ok(())
}

pub(crate) fn replace(
    store: &mut impl CredentialStore,
    value: &str,
    cleanup_legacy: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let previous = store.read().map_err(|_| {
        anyhow::anyhow!("Cannot inspect OS credential before replacement; no write attempted.")
    })?;
    let committed = write_verified(store, value).and_then(|()| cleanup_legacy());
    if committed.is_ok() {
        return Ok(());
    }
    let restored = match previous {
        Some(prior) => write_verified(store, &prior),
        None => delete_verified(store),
    };
    if restored.is_err() {
        bail!("{UNCERTAIN}");
    }
    bail!("Could not save connector credential; previous credential state restored.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Fake {
        value: Option<String>,
        reads: VecDeque<bool>,
        writes: VecDeque<bool>,
        deletes: VecDeque<bool>,
        ignore_delete: bool,
        mutations: usize,
    }

    fn outcome(queue: &mut VecDeque<bool>) -> Result<()> {
        if queue.pop_front().unwrap_or(true) {
            Ok(())
        } else {
            bail!("secret-backend-value")
        }
    }

    impl CredentialStore for Fake {
        fn read(&mut self) -> Result<Option<String>> {
            outcome(&mut self.reads)?;
            Ok(self.value.clone())
        }
        fn write(&mut self, value: &str) -> Result<()> {
            self.mutations += 1;
            // A write may take effect before the backend reports a failure.
            self.value = Some(value.into());
            outcome(&mut self.writes)
        }
        fn delete(&mut self) -> Result<()> {
            self.mutations += 1;
            outcome(&mut self.deletes)?;
            if !self.ignore_delete {
                self.value = None;
            }
            Ok(())
        }
    }

    #[test]
    fn successful_replacement_cleans_up_only_after_verification() {
        let mut store = Fake::default();
        let mut cleaned = false;
        replace(&mut store, "new", || {
            cleaned = true;
            Ok(())
        })
        .unwrap();
        assert!(cleaned);
        assert_eq!(store.value.as_deref(), Some("new"));
    }

    #[test]
    fn unreadable_previous_state_never_writes_or_cleans_up() {
        let mut store = Fake {
            reads: [false].into(),
            ..Default::default()
        };
        let err = replace(&mut store, "new", || panic!("cleanup before verification")).unwrap_err();
        assert_eq!(store.mutations, 0);
        assert!(!format!("{err:#}").contains("secret-backend-value"));
    }

    #[test]
    fn failed_write_and_readback_restore_previous_or_absent_state() {
        for previous in [None, Some("old".to_owned())] {
            for write_failed in [false, true] {
                let mut store = Fake {
                    value: previous.clone(),
                    ..Default::default()
                };
                if write_failed {
                    store.writes = [false, true].into();
                } else {
                    store.reads = [true, false, true].into();
                }
                let err = replace(&mut store, "new", || panic!("cleanup after failed write"))
                    .unwrap_err();
                assert!(err
                    .to_string()
                    .contains("previous credential state restored"));
                assert_eq!(store.value, previous);
                assert!(!format!("{err:#}").contains("secret-backend-value"));
            }
        }
    }

    #[test]
    fn cleanup_failure_restores_previous_and_never_exposes_backend_error() {
        let mut store = Fake {
            value: Some("old".into()),
            ..Default::default()
        };
        let err = replace(&mut store, "new", || bail!("secret-backend-value")).unwrap_err();
        assert_eq!(store.value.as_deref(), Some("old"));
        assert!(!format!("{err:#}").contains("secret-backend-value"));
    }

    #[test]
    fn every_failed_rollback_has_the_same_recovery_marker() {
        for cleanup_failed in [false, true] {
            for previous in [None, Some("old".to_owned())] {
                let mut store = Fake {
                    value: previous.clone(),
                    ..Default::default()
                };
                if !cleanup_failed {
                    store.reads = [true, false, false].into();
                } else {
                    store.reads = [true, true, false].into();
                }
                let err = replace(&mut store, "new", || bail!("secret-backend-value")).unwrap_err();
                assert_eq!(err.to_string(), UNCERTAIN);
            }
        }
    }

    #[test]
    fn revocation_requires_confirmed_absence() {
        let mut store = Fake {
            value: Some("old".into()),
            ignore_delete: true,
            ..Default::default()
        };
        assert!(
            delete_verified(&mut store).is_err(),
            "a lying backend must not report success"
        );
        store.ignore_delete = false;
        store.reads = [false].into();
        assert!(
            delete_verified(&mut store).is_err(),
            "an unreadable backend is not proof of absence"
        );
        delete_verified(&mut store).unwrap();
    }
}
