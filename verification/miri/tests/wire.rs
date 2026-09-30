// Run the actual vendored regressions, not copies that can drift from them.
#[path = "../../../vendor/capnp/tests/double_far_oob.rs"]
mod double_far_oob;
#[path = "../../../vendor/capnp/tests/far_pointer_oob.rs"]
mod far_pointer_oob;
#[path = "../../../vendor/capnp/tests/inline_composite_tag_oob.rs"]
mod inline_composite_tag_oob;
#[path = "../../../vendor/capnp/tests/negative_pointer_offset.rs"]
mod negative_pointer_offset;
#[path = "../../../vendor/capnp/tests/total_size.rs"]
mod total_size;
#[path = "../../../vendor/capnp/tests/zero_size_alloc.rs"]
mod zero_size_alloc;
