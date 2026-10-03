use capntproto::{
    semantics::Revisions,
    storage::{ObjectKey, Revision, Store},
};

#[test]
fn counter_import_preserves_zero_and_all_bits_with_explicit_numeric_encoding() {
    for value in [0, 1, 1 << 32, u64::MAX - 1, u64::MAX] {
        let revision = Revision::new(value);
        let wire = value.to_string();
        assert_eq!(revision.get(), value);
        assert_eq!(revision.to_string(), wire);
        assert_eq!(serde_json::to_string(&revision).unwrap(), wire);
        assert_eq!(serde_json::from_str::<Revision>(&wire).unwrap(), revision);
        assert_eq!(
            revision.checked_next().map(Revision::get),
            value.checked_add(1)
        );
    }
    for invalid in [
        "-1",
        "18446744073709551616",
        "1.0",
        "\"1\"",
        "null",
        "{}",
        "true",
    ] {
        assert!(
            serde_json::from_str::<Revision>(invalid).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn checked_revision_states_reject_inverted_bounds_and_preserve_rejected_inputs() {
    let values = [0, 1, 2, u64::MAX - 1, u64::MAX].map(Revision::new);
    for head in values {
        for published in values {
            let state = Revisions::new(head, published);
            assert_eq!(state.is_some(), published <= head);
            let Some(state) = state else { continue };
            for expected in values {
                let next = state.stage(expected);
                assert_eq!(next.is_some(), expected == head && head != Revision::MAX);
                if let Some(next) = next {
                    assert_eq!(next.head().get(), head.get() + 1);
                    assert_eq!(next.published(), published);
                }
                for revision in values {
                    let next = state.publish(revision, expected);
                    assert_eq!(
                        next.is_some(),
                        expected == published && revision > published && revision <= head
                    );
                    if let Some(next) = next {
                        assert_eq!(next.head(), head);
                        assert_eq!(next.published(), revision);
                    }
                }
                assert_eq!((state.head(), state.published()), (head, published));
            }
        }
    }
    assert_eq!(
        Revisions::default(),
        Revisions::new(Revision::INITIAL, Revision::INITIAL).unwrap()
    );
}

#[test]
fn initial_counter_is_not_an_entry_and_numeric_metadata_is_not_publication_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("objects")).unwrap();
    let key = ObjectKey::new(0);
    assert_eq!(store.head(key), Revision::INITIAL);
    assert_eq!(store.published(key), Revision::INITIAL);
    assert!(store.revision(key, Revision::INITIAL).is_err());
    assert!(store
        .publish(key, Revision::INITIAL, Revision::INITIAL)
        .is_err());
    let staged = store.put(key, Revision::INITIAL, b"draft").unwrap();
    assert_eq!(staged, Revision::new(1));
    assert_eq!(store.revision(key, staged).unwrap().revision(), staged);
    assert!(store.get(key).is_err());
    assert!(store.publication_cursor(key, staged.get()).is_err());
    store.publish(key, staged, Revision::INITIAL).unwrap();
    assert_eq!(
        store.publication_cursor(key, staged.get()).unwrap().after(),
        staged
    );
}
