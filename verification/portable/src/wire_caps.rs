use super::*;
use portable_capnp::{
    api::{
        echo::{CapabilityResults, ListResults},
        ChoiceBox,
    },
    echo,
};

pub fn bounded_wire_capabilities() -> Result<()> {
    let mut words = [ZERO; 32];
    let mut output = message::FixedBuilder::new(&mut words)?;
    {
        let mut root = output.try_init_root::<CapabilityResults>()?;
        root.field(CapabilityResults::VALUE).set_index(0)?;
        assert_eq!(root.read().value()?.index(), 0);
        root.field(CapabilityResults::VALUE).set_index(u32::MAX)?;
        assert_eq!(root.read().value()?.index(), u32::MAX);
        root.field(CapabilityResults::VALUE).clear();
        assert!(root.read().value().is_err());
        root.field(CapabilityResults::VALUE).set_index(1)?;
    }
    let content = output.get_root_as_reader::<capnp::any_pointer::Reader<'_>>()?;
    assert_eq!(
        content.validate_capability_indices(1).unwrap_err().kind,
        ErrorKind::MessageContainsInvalidCapabilityPointer
    );
    content.validate_capability_indices(2)?;
    assert_eq!(content.pointer_field(0)?.capability_index()?, 1);
    {
        let mut root = output.try_init_root::<ListResults>()?;
        let mut caps = root.field(ListResults::VALUES).init(3)?;
        for i in 0..3 {
            caps.get_mut(i).unwrap().set_index(i as u32)?;
        }
    }
    let content = output.get_root_as_reader::<capnp::any_pointer::Reader<'_>>()?;
    content.validate_capability_indices(3)?;
    assert!(content.validate_capability_indices(2).is_err());
    // A capability-bearing subtree cannot be copied without a descriptor map.
    let mut other = [ZERO; 32];
    let mut destination = message::FixedBuilder::new(&mut other)?;
    destination.try_set_root_text("unchanged")?;
    assert!(destination.try_copy_root(content).is_err());
    assert_eq!(
        destination.get_root_as_reader::<capnp::text::Reader<'_>>()?,
        "unchanged"
    );
    let mapping = capnp::wire_capability::IndexMap::new(&[2, 0, 1], 3)?;
    destination.try_copy_root_with_capabilities(content, mapping)?;
    {
        let root = destination.try_get_root::<ListResults>()?;
        let values = root.read().values()?;
        for (i, expected) in [2, 0, 1].into_iter().enumerate() {
            assert_eq!(values.get(i).unwrap()?.index(), expected);
        }
    }
    let size = destination.size_in_words();
    let incomplete = capnp::wire_capability::IndexMap::new(&[2, 0], 3)?;
    assert_eq!(
        destination
            .try_copy_root_with_capabilities(content, incomplete)
            .unwrap_err()
            .kind,
        ErrorKind::MessageContainsInvalidCapabilityPointer
    );
    assert_eq!(destination.size_in_words(), size);
    destination.reset();
    {
        type Choice = ChoiceBox<capnp::any_pointer::Owned>;
        let mut root = destination.try_init_root::<Choice>()?;
        root.scalar(Choice::COUNT).set(99);
        assert!(root
            .field(Choice::VALUE)
            .typed()
            .copy_with_capabilities(content, incomplete)
            .is_err());
        assert_eq!(root.read().count()?, 99);
        root.field(Choice::VALUE)
            .typed()
            .copy_with_capabilities(content, mapping)?;
        root.read().value()?.validate_capability_indices(3)?;
    }
    // Replacing a whole capability-bearing tree drops no hosted hooks, even
    // when the core's alloc feature is enabled in another target profile.
    output.try_set_root_text("replacement")?;
    output.reset();
    {
        let mut root = output.try_init_root::<ChoiceBox<echo::Owned>>()?;
        root.field(ChoiceBox::VALUE).typed().set_index(3)?;
        assert_eq!(root.read().value()?.index(), 3);
        root.scalar(ChoiceBox::COUNT).set(17);
        assert!(root.read().value().is_err());
    }
    output.reset();
    output.set_root_capability_index(0)?;
    assert_eq!(
        output
            .get_root_as_reader::<capnp::any_pointer::Reader<'_>>()?
            .capability_index()?,
        0
    );
    output.try_set_root_text("done")?;
    Ok(())
}
