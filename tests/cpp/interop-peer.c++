// Independent implementation of tests/interop/rpc_peer.rs's behavioral contract.
#include "interop.capnp.h"
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <iostream>
#include <limits>
#include <memory>
#include <string>
#include <vector>

struct State {
  uint32_t started = 0, canceled = 0, released = 0, ordered = 0;
  uint64_t streamed = 0;
  kj::Own<kj::PromiseFulfiller<Peer::Client>> pending;
};
struct Held {
  std::shared_ptr<State> state;
  explicit Held(std::shared_ptr<State> state): state(state) {}
  ~Held() { ++state->canceled; }
};
static kj::Exception::Type errorType(uint16_t kind) {
  switch (kind) {
    case 0: return kj::Exception::Type::FAILED;
    case 1: return kj::Exception::Type::OVERLOADED;
    case 2: return kj::Exception::Type::DISCONNECTED;
    case 3: return kj::Exception::Type::UNIMPLEMENTED;
    default: KJ_FAIL_REQUIRE("unknown exception type");
  }
}
class Server final: public Peer::Server {
public:
  explicit Server(std::shared_ptr<State> state, bool child = false): state(state), isChild(child) {}
  ~Server() { if (isChild) ++state->released; }
private:
  std::shared_ptr<State> state;
  bool isChild;
  kj::Promise<void> echo(EchoContext c) override {
    auto p = c.getParams(); auto r = c.getResults();
    r.setValue(p.getValue()); r.setPayload(p.getPayload()); r.setText(p.getText());
    return kj::READY_NOW;
  }
  kj::Promise<void> bounce(BounceContext c) override {
    c.getResults().setCap(c.getParams().getCap()); return kj::READY_NOW;
  }
  kj::Promise<void> child(ChildContext c) override {
    c.getResults().setCap(kj::heap<Server>(state, true)); return kj::READY_NOW;
  }
  kj::Promise<void> tail(TailContext c) override {
    auto p = c.getParams(); auto call = p.getCap().echoRequest(); call.setValue(p.getValue());
    return c.tailCall(kj::mv(call));
  }
  kj::Promise<void> callback(CallbackContext c) override {
    auto p = c.getParams(); auto call = p.getCap().tailRequest();
    call.setValue(p.getValue()); call.setCap(thisCap().castAs<Peer>());
    return call.send().then([c = kj::mv(c)](auto r) mutable { c.getResults().setValue(r.getValue()); });
  }
  kj::Promise<void> fail(FailContext c) override {
    return kj::Exception(errorType(c.getParams().getKind()), __FILE__, __LINE__, kj::str("interop failure"));
  }
  kj::Promise<void> hold(HoldContext) override {
    ++state->started;
    return kj::Promise<void>(kj::NEVER_DONE).attach(kj::heap<Held>(state));
  }
  kj::Promise<void> stats(StatsContext c) override {
    auto r = c.getResults(); r.setStarted(state->started); r.setCanceled(state->canceled);
    r.setReleased(state->released); r.setStreamed(state->streamed); return kj::READY_NOW;
  }
  kj::Promise<void> promise(PromiseContext c) override {
    KJ_REQUIRE(state->pending.get() == nullptr);
    auto pair = kj::newPromiseAndFulfiller<Peer::Client>();
    c.getResults().setCap(Peer::Client(kj::mv(pair.promise)));
    state->pending = kj::mv(pair.fulfiller); return kj::READY_NOW;
  }
  kj::Promise<void> resolve(ResolveContext c) override {
    KJ_REQUIRE(state->pending.get() != nullptr);
    if (c.getParams().getReject()) state->pending->reject(KJ_EXCEPTION(FAILED, "promise rejected"));
    else state->pending->fulfill(thisCap().castAs<Peer>());
    state->pending = nullptr; return kj::READY_NOW;
  }
  kj::Promise<void> stream(StreamContext c) override {
    state->streamed += c.getParams().getValue(); return kj::READY_NOW;
  }
  kj::Promise<void> order(OrderContext c) override {
    KJ_REQUIRE(c.getParams().getValue() == state->ordered, "E-order lost during reflection");
    ++state->ordered; return kj::READY_NOW;
  }
};
static void pass(const char* name) { std::cout << "PASS " << name << std::endl; }
static void runClient(Peer::Client remote, kj::WaitScope& wait) {
  for (size_t bytes: {0, 1, 7, 8, 1024, 65536, 262144, 1048576, 4194304}) {
    std::vector<capnp::byte> data(bytes);
    for (size_t i = 0; i < bytes; ++i) data[i] = i % 251;
    auto call = remote.echoRequest(); auto value = std::numeric_limits<uint64_t>::max() - bytes;
    const char text[] = "Grüße 🌍\0end";
    call.setValue(value); call.setPayload(kj::arrayPtr(data.data(), data.size()));
    call.setText(kj::StringPtr(text, sizeof(text) - 1));
    auto r = call.send().wait(wait);
    KJ_REQUIRE(r.getValue() == value);
    KJ_REQUIRE(r.getPayload() == kj::arrayPtr(data.data(), data.size()));
    KJ_REQUIRE(r.getText() == kj::StringPtr(text, sizeof(text) - 1));
  }
  pass("bootstrap-payloads");
  auto state = std::make_shared<State>();
  Peer::Client local(kj::heap<Server>(state));
  auto bounce = remote.bounceRequest(); bounce.setCap(local);
  auto response = bounce.send();
  auto pipelined = response.getCap();
  auto early = pipelined.orderRequest(); early.setValue(0);
  auto first = early.send();
  auto returned = response.wait(wait).getCap();
  auto later = pipelined.orderRequest(); later.setValue(1); later.send().wait(wait);
  first.wait(wait); KJ_REQUIRE(state->ordered == 2);
  returned.echoRequest().send().wait(wait);
  pass("pipeline-reflection-embargo");
  auto tail = remote.tailRequest(); tail.setCap(local); tail.setValue(42);
  KJ_REQUIRE(tail.send().wait(wait).getValue() == 42);
  pass("tail-to-callback");
  auto callback = remote.callbackRequest(); callback.setCap(local); callback.setValue(43);
  KJ_REQUIRE(callback.send().wait(wait).getValue() == 43);
  pass("bidirectional-tail-calls");
  for (bool reject: {false, true}) {
    auto promise = remote.promiseRequest().send().wait(wait).getCap();
    auto call = promise.echoRequest(); call.setValue(44); auto pending = call.send();
    auto resolve = remote.resolveRequest(); resolve.setReject(reject); resolve.send().wait(wait);
    if (reject) {
      auto error = kj::runCatchingExceptions([&]() { pending.wait(wait); });
      KJ_REQUIRE(error != kj::none);
      KJ_IF_SOME(e, error) {
        KJ_REQUIRE(e.getType() == kj::Exception::Type::FAILED);
        KJ_REQUIRE(std::string(e.getDescription().cStr()).find("promise rejected") != std::string::npos);
      }
      KJ_REQUIRE(kj::runCatchingExceptions([&]() { promise.echoRequest().send().wait(wait); }) != kj::none);
    } else { KJ_REQUIRE(pending.wait(wait).getValue() == 44); }
  }
  pass("promise-resolution-rejection");
  for (uint16_t kind = 0; kind < 4; ++kind) {
    auto call = remote.failRequest(); call.setKind(kind);
    auto error = kj::runCatchingExceptions([&]() { call.send().wait(wait); });
    KJ_REQUIRE(error != kj::none);
    KJ_IF_SOME(e, error) {
      KJ_REQUIRE(e.getType() == errorType(kind));
      KJ_REQUIRE(std::string(e.getDescription().cStr()).find("interop failure") != std::string::npos);
    }
  }
  for (bool unknownInterface: {false, true}) {
    auto call = remote.typelessRequest(unknownInterface ? 0xeabcffffffffffffull : capnp::typeId<Peer>(),
        unknownInterface ? 0 : 65535, kj::none, {});
    auto error = kj::runCatchingExceptions([&]() { call.send().wait(wait); });
    KJ_REQUIRE(error != kj::none);
    KJ_IF_SOME(e, error) { KJ_REQUIRE(e.getType() == kj::Exception::Type::UNIMPLEMENTED); }
  }
  pass("exceptions-unknown-methods");
  {
    auto held = remote.holdRequest().send();
    while (remote.statsRequest().send().wait(wait).getStarted() != 1) {}
  }
  while (remote.statsRequest().send().wait(wait).getCanceled() != 1) {}
  pass("finish-cancellation");
  {
    auto child = remote.childRequest().send().wait(wait).getCap();
    child.echoRequest().send().wait(wait);
  }
  while (remote.statsRequest().send().wait(wait).getReleased() != 1) {}
  pass("capability-release");
  for (uint64_t value = 1; value <= 32; ++value) {
    auto call = remote.streamRequest(); call.setValue(value); call.send().wait(wait);
  }
  KJ_REQUIRE(remote.statsRequest().send().wait(wait).getStreamed() == 528);
  pass("streaming");
  remote.echoRequest().send().wait(wait); pass("connection-survives-errors");
}
int main(int argc, char** argv) {
  KJ_REQUIRE(argc >= 3, "driver mode [address]");
  auto io = kj::setupAsyncIo();
  if (std::string(argv[2]) == "server") {
    auto address = io.provider->getNetwork().parseAddress("127.0.0.1", 0).wait(io.waitScope);
    auto listener = address->listen();
    std::cout << listener->getPort() << std::endl;
    auto socket = listener->accept().wait(io.waitScope);
    capnp::TwoPartyClient rpc(*socket, Peer::Client(kj::heap<Server>(std::make_shared<State>())), capnp::rpc::twoparty::Side::SERVER);
    rpc.onDisconnect().wait(io.waitScope);
  } else {
    KJ_REQUIRE(argc == 4 && std::string(argv[2]) == "client");
    auto address = io.provider->getNetwork().parseAddress(argv[3]).wait(io.waitScope);
    auto socket = address->connect().wait(io.waitScope);
    { capnp::TwoPartyClient rpc(*socket); runClient(rpc.bootstrap().castAs<Peer>(), io.waitScope); }
    socket->shutdownWrite(); pass("graceful-shutdown");
  }
}
