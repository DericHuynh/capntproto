"""Prevent concrete owners from being mistaken for hosted defaults in reports."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "owner_names", Path(__file__).with_name("canonicalize-owner-types.py")
)
owner_names = importlib.util.module_from_spec(spec)
spec.loader.exec_module(owner_names)


class OwnerNames(unittest.TestCase):
    def test_only_exact_default_client_is_collapsed(self):
        self.assertEqual(owner_names.canonical(
            "capnp::capability::Client<alloc::boxed::Box<dyn capnp::private::capability::ClientHook>>"
        ), "capnp::capability::Client")
        borrowed = "capnp::capability::Client<&dyn capnp::private::capability::ClientHook>"
        self.assertEqual(owner_names.canonical(borrowed), borrowed)

    def test_custom_future_stays_distinct(self):
        borrowed = "capnp::capability::Promise<T, E, core::pin::Pin<&mut F>>"
        self.assertEqual(owner_names.canonical(borrowed), borrowed)

    def test_exact_hosted_reply_alias_with_array_vat(self):
        response = "capnp_rpc::rpc::Response<[u8; 32]>"
        promise = (f"capnp::capability::Promise<{response}, capnp::Error, "
                   "core::pin::Pin<alloc::boxed::Box<dyn core::future::future::Future<"
                   f"Output = core::result::Result<{response}, capnp::Error>>>>>")
        receiver = ("capntproto_rpc_engine::completion::direct::receiver::Receiver<"
                    "alloc::rc::Rc<capnp_rpc::rpc::QuestionCell<[u8; 32]>>, " + promise + ">")
        self.assertEqual(owner_names.canonical(receiver), "capnp_rpc::rpc::QuestionReply<[u8; 32]>")

    def test_borrowed_reply_and_nondefault_payload_stay_distinct(self):
        borrowed = "capntproto_rpc_engine::completion::direct::receiver::Receiver<&Context, Future>"
        self.assertEqual(owner_names.canonical(borrowed), borrowed)
        other = ("capntproto_rpc_engine::completion::direct::receiver::Receiver<"
                 "alloc::rc::Rc<capnp_rpc::rpc::QuestionCell<Vat>>, Future>")
        self.assertEqual(owner_names.canonical(other), other)


if __name__ == "__main__":
    unittest.main()
