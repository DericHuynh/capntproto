// Run the actual vendored regressions, not copies that can drift from them.
#[path = "../../../crates/capntproto-core/tests/double_far_oob.rs"]
mod double_far_oob;
#[path = "../../../crates/capntproto-core/tests/far_pointer_oob.rs"]
mod far_pointer_oob;
#[path = "../../../crates/capntproto-core/tests/inline_composite_tag_oob.rs"]
mod inline_composite_tag_oob;
#[path = "../../../crates/capntproto-core/tests/negative_pointer_offset.rs"]
mod negative_pointer_offset;
#[path = "../../../crates/capntproto-core/tests/total_size.rs"]
mod total_size;
#[path = "../../../crates/capntproto-core/tests/zero_size_alloc.rs"]
mod zero_size_alloc;
