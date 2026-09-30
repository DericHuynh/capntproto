#include "cancellation-policy.capnp.h"
#include "runtime-test.capnp.h"
#include <capnp/capability.h>
#include <kj/async.h>
#include <kj/debug.h>
#include <iostream>
#ifdef REPROTO_PINNED_RUNTIME
#include "dynamic-test.capnp.h"
#include <capnp/dynamic.h>
#include <capnp/message.h>
#endif

struct State {
  bool running = false;
  bool completed = false;
  bool alive = false;
  kj::Own<kj::PromiseFulfiller<void>> gate;
};
class Value final: public Harness::Server {
public:
  explicit Value(State& state): state(state) { state.alive = true; }
  ~Value() { state.alive = false; }
private:
  State& state;
  kj::Promise<void> echo(EchoContext c) override { c.getResults().setValue(42); return kj::READY_NOW; }
};
struct Running {
  explicit Running(State& state): state(state) {}
  State& state;
  ~Running() { state.running = false; }
};
template <typename Context>
kj::Promise<void> run(State& state, Context context) {
  state.running = true;
  context.getResults().setCap(kj::heap<Value>(state));
  auto gate = kj::newPromiseAndFulfiller<void>();
  state.gate = kj::mv(gate.fulfiller);
  // Release the application's context before completion. The runtime must keep
  // its result capability until the non-cancellable application promise ends.
  return gate.promise.then([&state]() { state.completed = true; })
      .attach(kj::heap<Running>(state));
}
class PolicyServer final: public Policy::Server {
public:
  explicit PolicyServer(State& state): state(state) {}
private:
  State& state;
  kj::Promise<void> pending(PendingContext c) override { return run(state, kj::mv(c)); }
  kj::Promise<void> cancellable(CancellableContext c) override { return run(state, kj::mv(c)); }
};
class AllowedServer final: public Allowed::Server {
public:
  explicit AllowedServer(State& state): state(state) {}
private:
  State& state;
  kj::Promise<void> pending(PendingContext c) override { return run(state, kj::mv(c)); }
};
class DerivedServer final: public Derived::Server {
public:
  explicit DerivedServer(State& state): state(state) {}
private:
  State& state;
  kj::Promise<void> pending(PendingContext c) override { return run(state, kj::mv(c)); }
  kj::Promise<void> cancellable(CancellableContext c) override { return run(state, kj::mv(c)); }
  kj::Promise<void> pendingOwn(PendingOwnContext c) override { return run(state, kj::mv(c)); }
};
class FileServer final: public Harness::Server {
public:
  explicit FileServer(State& state): state(state) {}
private:
  State& state;
  kj::Promise<void> pending(PendingContext c) override { return run(state, kj::mv(c)); }
};
template <typename Server>
void check(kj::WaitScope& wait, uint64_t interfaceId, uint16_t method, bool allow) {
  State state;
  capnp::Capability::Client client(kj::heap<Server>(state));
  {
    auto promise = client.typelessRequest(interfaceId, method, nullptr, {}).send();
    KJ_REQUIRE(!promise.poll(wait));
    KJ_REQUIRE(state.alive);
  }
  for (int i=0; i<8; ++i) kj::evalLater([]() {}).wait(wait);
  KJ_REQUIRE(state.running == !allow, allow, state.running);
  KJ_REQUIRE(state.alive == !allow, allow, state.alive);
  if (!allow) {
    state.gate->fulfill();
    for (int i=0; i<8; ++i) kj::evalLater([]() {}).wait(wait);
    KJ_REQUIRE(state.completed && !state.running && !state.alive);
  }
}
#ifdef REPROTO_PINNED_RUNTIME
void checkRevocable(kj::WaitScope& wait) {
  for (bool allow: {false, true}) {
    State state;
    PolicyServer server(state);
    capnp::RevocableServer<Policy> owner(server);
    KJ_REQUIRE(!owner.isInUse());
    auto request = owner.getClient().typelessRequest(capnp::typeId<Policy>(), allow ? 1 : 0, nullptr, {});
    auto pending = request.send();
    KJ_REQUIRE(!pending.poll(wait));
    KJ_REQUIRE(owner.isInUse() && state.running);
    owner.revoke(KJ_EXCEPTION(OVERLOADED, "owner revoked"));
    KJ_REQUIRE(!state.running);
    bool rejected = false;
    try { pending.wait(wait); } catch (const kj::Exception& e) {
      rejected = e.getType() == kj::Exception::Type::OVERLOADED && e.getDescription().contains("owner revoked");
    }
    KJ_REQUIRE(rejected && !state.completed && !state.alive);
    owner.revoke(KJ_EXCEPTION(FAILED, "second reason"));
    rejected = false;
    try { owner.getClient().pendingRequest().send().wait(wait); } catch (const kj::Exception& e) {
      rejected = e.getType() == kj::Exception::Type::OVERLOADED;
    }
    KJ_REQUIRE(rejected);
  }
  State state;
  PolicyServer server(state);
  capnp::RevocableServer<Policy> owner(server);
  auto pending = owner.getClient().pendingRequest().send();
  KJ_REQUIRE(!pending.poll(wait));
  state.gate->fulfill();
  auto response = pending.wait(wait);
  auto cap = response.getCap();
  owner.revoke();
  KJ_REQUIRE(cap.echoRequest().send().wait(wait).getValue() == 42);
}
class StreamServer final: public Harness::Server {
public:
  kj::Own<kj::PromiseFulfiller<void>> gates[2];
  uint started = 0;
private:
  kj::Promise<void> stream(StreamContext) override {
    auto gate = kj::newPromiseAndFulfiller<void>();
    gates[started++] = kj::mv(gate.fulfiller);
    return kj::mv(gate.promise);
  }
};
void checkServerSet(kj::WaitScope& wait) {
  capnp::CapabilityServerSet<Harness> set;
  auto server = kj::heap<StreamServer>();
  auto& ref = *server;
  auto client = set.add(kj::mv(server));
  auto a = client.streamRequest().send();
  KJ_REQUIRE(!a.poll(wait));
  KJ_REQUIRE(set.tryGetLocalServerSync(client) == kj::none);
  auto lookup = set.getLocalServer(client);
  KJ_REQUIRE(!lookup.poll(wait));
  auto b = client.streamRequest().send();
  KJ_REQUIRE(!b.poll(wait));
  ref.gates[0]->fulfill();
  a.wait(wait);
  KJ_REQUIRE(lookup.wait(wait) != kj::none);
  KJ_REQUIRE(!b.poll(wait));
  KJ_REQUIRE(set.tryGetLocalServerSync(client) == kj::none);
  ref.gates[1]->fulfill();
  b.wait(wait);
  KJ_REQUIRE(set.tryGetLocalServerSync(client) != kj::none);
}
class ShortServer final: public Harness::Server, public kj::Refcounted {
public:
  explicit ShortServer(kj::Promise<capnp::Capability::Client> resolution)
      : resolution(kj::mv(resolution)) {}
  Harness::Client self() { return thisCap(); }
  kj::Own<kj::PromiseFulfiller<void>> gates[2];
  uint started = 0;
private:
  kj::Maybe<kj::Promise<capnp::Capability::Client>> resolution;
  kj::Maybe<kj::Promise<capnp::Capability::Client>> shortenPath() override {
    return kj::mv(resolution);
  }
  kj::Promise<void> stream(StreamContext) override {
    auto gate = kj::newPromiseAndFulfiller<void>();
    gates[started++] = kj::mv(gate.fulfiller);
    return kj::mv(gate.promise);
  }
  kj::Promise<void> echo(EchoContext c) override {
    c.getResults().setValue(1);
    return kj::READY_NOW;
  }
};
void checkServerHooks(kj::WaitScope& wait) {
  {
    auto resolution = kj::newPromiseAndFulfiller<capnp::Capability::Client>();
    auto server = kj::refcounted<ShortServer>(kj::mv(resolution.promise));
    Harness::Client a(kj::addRef(*server));
    Harness::Client b(kj::addRef(*server));
    auto self = server->self();
    KJ_REQUIRE(capnp::ClientHook::from(a).get() == capnp::ClientHook::from(b).get());
    KJ_REQUIRE(capnp::ClientHook::from(a).get() == capnp::ClientHook::from(self).get());
    resolution.fulfiller->reject(KJ_EXCEPTION(FAILED, "shortening failed"));
    bool failed = false;
    try { a.whenResolved().wait(wait); } catch (const kj::Exception& e) {
      failed = e.getDescription().contains("shortening failed");
    }
    KJ_REQUIRE(failed);
    // Failed resolution rejects promise observers but preserves the local route.
    KJ_REQUIRE(a.echoRequest().send().wait(wait).getValue() == 1);
  }
  for (bool revoke: {false, true}) {
    State targetState;
    auto resolution = kj::newPromiseAndFulfiller<capnp::Capability::Client>();
    ShortServer server(kj::mv(resolution.promise));
    capnp::RevocableServer<Harness> owner(server);
    auto client = owner.getClient();
    auto a = client.streamRequest().send();
    KJ_REQUIRE(!a.poll(wait));
    auto b = server.self().streamRequest().send();
    KJ_REQUIRE(!b.poll(wait) && server.started == 1);
    resolution.fulfiller->fulfill(kj::heap<Value>(targetState));
    auto resolved = client.whenResolved();
    KJ_REQUIRE(!resolved.poll(wait));
    auto c = client.echoRequest().send();
    KJ_REQUIRE(!c.poll(wait));
    server.gates[0]->fulfill();
    a.wait(wait);
    KJ_REQUIRE(!c.poll(wait) && server.started == 2);
    if (revoke) {
      owner.revoke();
      bool failed = false;
      try { b.wait(wait); } catch (const kj::Exception&) { failed = true; }
      KJ_REQUIRE(failed);
    } else {
      server.gates[1]->fulfill();
      b.wait(wait);
    }
    resolved.wait(wait);
    KJ_REQUIRE(c.wait(wait).getValue() == 42);
    owner.revoke();
    KJ_REQUIRE(client.echoRequest().send().wait(wait).getValue() == 42);
  }
}

void checkShortenLookup(kj::WaitScope& wait) {
  for (bool reject: {false, true}) {
    State targetState;
    auto resolution = kj::newPromiseAndFulfiller<capnp::Capability::Client>();
    auto server = kj::refcounted<ShortServer>(kj::mv(resolution.promise));
    auto& oldServer = *server;
    capnp::CapabilityServerSet<Harness> set;
    auto client = set.add(kj::mv(server));
    auto targetServer = kj::heap<Value>(targetState);
    auto& targetRef = *targetServer;
    auto target = set.add(kj::mv(targetServer));
    // A known local member is available while its shortening promise is pending.
    KJ_REQUIRE(&KJ_ASSERT_NONNULL(set.getLocalServer(client).wait(wait)) == &oldServer);
    auto stream = client.streamRequest().send();
    KJ_REQUIRE(!stream.poll(wait));
    auto lookup = set.getLocalServer(client);
    KJ_REQUIRE(!lookup.poll(wait));
    if (reject) {
      resolution.fulfiller->reject(KJ_EXCEPTION(FAILED, "shortening failed"));
    } else {
      resolution.fulfiller->fulfill(kj::mv(target));
    }
    for (int i=0; i<8; ++i) kj::evalLater([]() {}).wait(wait);
    KJ_REQUIRE(set.tryGetLocalServerSync(client) == kj::none);
    oldServer.gates[0]->fulfill();
    stream.wait(wait);
    KJ_REQUIRE(&KJ_ASSERT_NONNULL(lookup.wait(wait)) == &oldServer);
    for (int i=0; i<8; ++i) kj::evalLater([]() {}).wait(wait);
    Harness::Server* expected = reject ? static_cast<Harness::Server*>(&oldServer)
                                      : static_cast<Harness::Server*>(&targetRef);
    KJ_REQUIRE(&KJ_ASSERT_NONNULL(set.tryGetLocalServerSync(client)) == expected);
    KJ_REQUIRE(&KJ_ASSERT_NONNULL(set.getLocalServer(client).wait(wait)) == expected);
  }
}

class DynamicServer final: public capnp::DynamicCapability::Server {
public:
  DynamicServer(State& state, bool allow): capnp::DynamicCapability::Server(
      capnp::Schema::from<dynamicTest::Derived<Harness>>(), Options{.allowCancellation=allow}), state(state) {}
  kj::Promise<void> call(capnp::InterfaceSchema::Method method,
      capnp::CallContext<capnp::DynamicStruct, capnp::DynamicStruct> context) override {
    KJ_REQUIRE(method.getProto().getName() == "pending");
    context.getResults().set("cap", context.getParams().get("cap"));
    context.releaseParams();
    state.running = true;
    auto gate = kj::newPromiseAndFulfiller<void>();
    state.gate = kj::mv(gate.fulfiller);
    return gate.promise.then([this]() { state.completed = true; })
        .attach(kj::heap<Running>(state));
  }
private:
  State& state;
};
void checkDynamicCapabilities(kj::WaitScope& wait) {
  auto derivedSchema = capnp::Schema::from<dynamicTest::Derived<Harness>>();
  auto baseSchema = capnp::Schema::from<dynamicTest::Base<Harness>>();
  KJ_REQUIRE(derivedSchema.extends(baseSchema));
  KJ_REQUIRE(derivedSchema.getMethodByName("pending").getContainingInterface() == baseSchema);
  for (bool allow: {false,true}) {
    State state, target;
    capnp::DynamicCapability::Client client(kj::heap<DynamicServer>(state,allow));
    {
      auto request = client.newRequest("pending");
      {
        Harness::Client value(kj::heap<Value>(target));
        request.set("cap", capnp::toDynamic(value));
      }
      auto call = request.send();
      KJ_REQUIRE(!call.poll(wait) && state.running && target.alive);
    }
    for (int i=0; i<8; ++i) kj::evalLater([]() {}).wait(wait);
    // The schema allows cancellation, but dynamic servers use their own option.
    KJ_REQUIRE(state.running == !allow && target.alive == !allow);
    if (!allow) {
      state.gate->fulfill();
      for (int i=0; i<8; ++i) kj::evalLater([]() {}).wait(wait);
      KJ_REQUIRE(state.completed && !state.running && !target.alive);
    }
  }
  State state, target;
  capnp::DynamicCapability::Client client(kj::heap<DynamicServer>(state,true));
  kj::Maybe<capnp::DynamicCapability::Client> retained;
  {
    auto request = client.newRequest("pending");
    {
      Harness::Client value(kj::heap<Value>(target));
      request.set("cap", capnp::toDynamic(value));
    }
    auto call = request.send();
    KJ_REQUIRE(!call.poll(wait));
    state.gate->fulfill();
    auto response = call.wait(wait);
    retained = response.get("cap").as<capnp::DynamicCapability>();
  }
  KJ_REQUIRE(target.alive);
  auto response = KJ_ASSERT_NONNULL(retained).newRequest("echo").send().wait(wait);
  KJ_REQUIRE(response.get("value").as<uint32_t>() == 42);
  retained = kj::none;
  KJ_REQUIRE(!target.alive);
}

void checkCapabilityReplacement() {
  State first, second;
  capnp::MallocMessageBuilder message;
  auto root = message.initRoot<dynamicTest::Parcel<Harness>>();
  root.setValue(kj::heap<Value>(first));
  KJ_REQUIRE(first.alive);
  root.setValue(kj::heap<Value>(second));
  KJ_REQUIRE(!first.alive && second.alive);
  message.initRoot<dynamicTest::Parcel<Harness>>();
  KJ_REQUIRE(!second.alive);
}
void checkDynamicBrands() {
  using TextBase = dynamicTest::Base<capnp::Text>;
  using DataBase = dynamicTest::Base<capnp::Data>;
  auto textBase = capnp::Schema::from<TextBase>();
  auto dataBase = capnp::Schema::from<DataBase>();
  KJ_REQUIRE(!textBase.extends(dataBase));
  KJ_REQUIRE(capnp::Schema::from<dynamicTest::Derived<capnp::Text>>().extends(textBase));
  KJ_REQUIRE(!capnp::Schema::from<dynamicTest::Derived<capnp::Text>>().extends(dataBase));
  KJ_REQUIRE(capnp::Schema::from<dynamicTest::Marker<capnp::Text>>() !=
             capnp::Schema::from<dynamicTest::Marker<capnp::Data>>());
  KJ_REQUIRE(capnp::Schema::from<dynamicTest::Wrap<capnp::Text>>().extends(
      capnp::Schema::from<dynamicTest::Base<dynamicTest::Parcel<capnp::Text>>>()));
  KJ_REQUIRE(!capnp::Schema::from<dynamicTest::Wrap<capnp::Text>>().extends(
      capnp::Schema::from<dynamicTest::Base<dynamicTest::Parcel<capnp::Data>>>()));
  using A = dynamicTest::Scope<capnp::Text>::Inner<capnp::Data>;
  using B = dynamicTest::Scope<capnp::Data>::Inner<capnp::Text>;
  KJ_REQUIRE(capnp::Schema::from<A>() != capnp::Schema::from<B>());
  TextBase::Client text(nullptr);
  auto reflected = capnp::toDynamic(text);
  // Native recovery checks the generic schema, erasing type parameters.
  auto native = reflected.as<DataBase>();
  KJ_REQUIRE(kj::runCatchingExceptions([&]() { reflected.upcast(dataBase); }) != kj::none);
  KJ_REQUIRE(kj::runCatchingExceptions([&]() {
    reflected.newRequest(dataBase.getMethodByName("transform"));
  }) != kj::none);
  capnp::MallocMessageBuilder message;
  auto root = capnp::toDynamic(message.initRoot<dynamicTest::BrandEnvelope>());
  root.set("cap", reflected);
  DataBase::Client data(nullptr);
  KJ_REQUIRE(kj::runCatchingExceptions([&]() { root.set("cap", capnp::toDynamic(data)); }) != kj::none);
  capnp::MallocMessageBuilder parcel;
  auto bad = parcel.initRoot<dynamicTest::Parcel<capnp::Data>>();
  KJ_REQUIRE(kj::runCatchingExceptions([&]() { root.set("item", capnp::toDynamic(bad.asReader())); }) != kj::none);
}

#endif
int main() {
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  check<PolicyServer>(wait, capnp::typeId<Policy>(), 0, false);
  check<PolicyServer>(wait, capnp::typeId<Policy>(), 1, true);
  check<AllowedServer>(wait, capnp::typeId<Allowed>(), 0, true);
  check<DerivedServer>(wait, capnp::typeId<Policy>(), 0, false);
  check<DerivedServer>(wait, capnp::typeId<Derived>(), 0, true);
  check<FileServer>(wait, capnp::typeId<Harness>(), 3, true);
#ifdef REPROTO_PINNED_RUNTIME
  checkRevocable(wait);
  checkServerSet(wait);
  checkServerHooks(wait);
  checkShortenLookup(wait);
  checkDynamicCapabilities(wait);
  checkDynamicBrands();
  checkCapabilityReplacement();
  std::cout << "Pinned C++ cancellation, revocation, server-set barriers, self-reference, shortenPath dynamic capabilities and generic brands passed\n";
#else
  std::cout << "C++ static cancellation: default, method/interface/file annotations, inherited policy and retained result capabilities passed\n";
#endif
}
