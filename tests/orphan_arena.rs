use capnp::{dynamic_struct, dynamic_value as value};
use reproto_test_support::dynamic_test_capnp::external_case;

#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u32>,
}
#[derive(serde::Deserialize)]
struct Case {
    steps: Vec<Step>,
}
fn run(case: &Case) -> capnp::Result<()> {
    let mut message =
        capnp::message::Builder::new(capnp::message::HeapAllocator::new().first_segment_words(128));
    message.init_root::<external_case::Builder>();
    let base: usize = message
        .get_segments_for_output()
        .iter()
        .map(|s| s.len() / 8)
        .sum();
    {
        let (mut root, token) = value::Builder::from(message.get_root::<external_case::Builder>()?)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        let mut data = access.new_data(24)?;
        let original = access.edit(&mut data, |v| {
            let bytes = v.downcast::<capnp::data::Builder>();
            bytes.fill(85);
            Ok(bytes.as_ptr())
        })?;
        let mut spacer = None;
        for step in &case.steps {
            match step.action.as_str() {
                "shrink" => access.resize(&mut data, 8)?,
                "grow" => access.resize(&mut data, 40)?,
                "spacer" => {
                    let mut other = access.new_data(8)?;
                    access.edit(&mut other, |v| {
                        let bytes = v.downcast::<capnp::data::Builder>();
                        assert_eq!(bytes, &[0; 8]);
                        bytes.fill(170);
                        Ok(())
                    })?;
                    spacer = Some(other);
                }
                action => panic!("unknown action {action}"),
            }
            let s = &step.state;
            access.read(&mut data, |v| {
                let bytes = v.downcast::<capnp::data::Reader>();
                assert_eq!(bytes.len(), s[0] as usize);
                assert_eq!(bytes.as_ptr() == original, s[11] == 1);
                assert!(bytes[..s[12] as usize].iter().all(|b| *b == 85));
                assert!(bytes[s[12] as usize..].iter().all(|b| *b == 0));
                Ok(())
            })?;
            if let Some(spacer) = &mut spacer {
                access.read(spacer, |v| {
                    assert_eq!(v.downcast::<capnp::data::Reader>(), &[170; 8]);
                    Ok(())
                })?;
            }
        }
    }
    // Each TLC edge is a separate prefix: check physical arena accounting at
    // its final state after releasing the borrow. Dropping owners does not
    // change segment allocation, so this observes the actual post-step extent.
    let words: usize = message
        .get_segments_for_output()
        .iter()
        .map(|s| s.len() / 8)
        .sum();
    assert_eq!(
        words - base,
        case.steps.last().map_or(4, |s| s.state[5] as usize)
    );
    Ok(())
}

#[test]
fn reclaimed_tail_is_reused_without_corrupting_retained_bytes() -> capnp::Result<()> {
    run(&Case {
        steps: vec![
            Step {
                action: "shrink".into(),
                state: vec![8, 1, 1, 0, 0, 2, 0, 2, 85, 1, 0, 1, 8],
            },
            Step {
                action: "spacer".into(),
                state: vec![8, 0, 1, 0, 1, 4, 0, 2, 85, 1, 0, 1, 8],
            },
            Step {
                action: "grow".into(),
                state: vec![40, 1, 1, 1, 1, 10, 6, 2, 85, 1, 1, 2, 8],
            },
        ],
    })
}

#[test]
fn replay_tlc_arena_resize_traces() -> capnp::Result<()> {
    let path = reproto_test_support::verification::input("REPROTO_ARENA_RESIZE_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for case in &cases {
        run(case)?;
    }
    Ok(())
}
