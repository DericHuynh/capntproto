#include <capnp/capability.h>
#include <capnp/membrane.h>
#include <kj/async.h>
#include <kj/debug.h>
#include <iostream>
#include <string>
#include <typeinfo>

class Service final: public capnp::Capability::Server {
  DispatchCallResult dispatchCall(uint64_t, uint16_t,
      capnp::CallContext<capnp::AnyPointer, capnp::AnyPointer>) override {
    KJ_FAIL_REQUIRE("diagnostics dispatched a call");
  }
};
class Boundary final: public capnp::MembranePolicy, public kj::Refcounted {
  kj::Maybe<capnp::Capability::Client> inboundCall(uint64_t, uint16_t,
      capnp::Capability::Client) override { KJ_FAIL_REQUIRE("policy called"); }
  kj::Maybe<capnp::Capability::Client> outboundCall(uint64_t, uint16_t,
      capnp::Capability::Client) override { KJ_FAIL_REQUIRE("policy called"); }
  kj::Own<capnp::MembranePolicy> addRef() override { return kj::addRef(*this); }
};

// Normalize language/compiler-specific type names and exception formatting only.
// The order and kind of every semantic wrapper must agree with Rust.
std::string normalized(capnp::Capability::Client& client) {
  auto description = client.debugInfo();
  std::string text(description.cStr()), result;
  std::string policy = std::string(typeid(Boundary).name()) + ":";
  for (;;) {
    if (text.starts_with(policy)) {
      result += "boundary:";
      text.erase(0, policy.size());
    } else if (text.starts_with("resolved:")) {
      result += "resolved:";
      text.erase(0, 9);
    } else if (text.starts_with("local:")) {
      KJ_REQUIRE(text.find(typeid(Service).name()) != std::string::npos);
      return result + "local";
    } else if (text.starts_with("broken:")) {
      KJ_REQUIRE(text.find("reason") != std::string::npos);
      return result + "broken";
    } else {
      KJ_REQUIRE((text == "promise"), text.c_str());
      return result + text;
    }
  }
}
int main() {
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  for (int depth = 0; depth <= 3; ++depth) {
    for (bool reject: {false, true}) {
      Service server;
      capnp::RevocableServer<capnp::Capability> owner(server);
      auto completion = kj::newPromiseAndFulfiller<capnp::Capability::Client>();
      capnp::Capability::Client promise(kj::mv(completion.promise));
      capnp::Capability::Client wrapped = promise;
      for (int i = 0; i < depth; ++i) {
        wrapped = capnp::membrane(kj::mv(wrapped), kj::refcounted<Boundary>());
      }
      auto observe = [&](int phase) {
        std::cout << depth << ' ' << reject << ' ' << phase << ' ' << normalized(wrapped) << '\n';
      };
      observe(0);
      if (reject) completion.fulfiller->reject(KJ_EXCEPTION(FAILED, "reason"));
      else completion.fulfiller->fulfill(owner.getClient());
      observe(1);
      auto error = kj::runCatchingExceptions([&]() { promise.whenResolved().wait(wait); });
      KJ_REQUIRE((error != kj::none) == reject);
      observe(2);
      owner.revoke(KJ_EXCEPTION(FAILED, "reason"));
      observe(3);
    }
  }
}
