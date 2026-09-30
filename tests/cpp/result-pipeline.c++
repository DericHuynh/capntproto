#include "runtime-test.capnp.h"
#include <capnp/capability.h>
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <optional>
#include <sstream>
#include <vector>

struct Echo final: Harness::Server {
  explicit Echo(unsigned& calls): calls(calls) {}
  unsigned& calls;
  kj::Promise<void> echo(EchoContext c) override {
    ++calls;
    c.getResults().setValue(73);
    return kj::READY_NOW;
  }
};
struct State {
  std::optional<capnp::CallContext<Harness::PendingParams, Harness::PendingResults>> context;
  kj::Own<kj::PromiseFulfiller<bool>> gate;
};
struct Controlled final: Harness::Server {
  explicit Controlled(State& state): state(state) {}
  State& state;
  kj::Promise<void> pending(PendingContext c) override {
    state.context.emplace(kj::mv(c));
    auto gate = kj::newPromiseAndFulfiller<bool>();
    state.gate = kj::mv(gate.fulfiller);
    return gate.promise.then([](bool ok) {
      KJ_REQUIRE(ok, "late failure");
    });
  }
};
void drain(kj::WaitScope& wait) {
  for (unsigned i=0; i<64; ++i) kj::evalLater([]() {}).wait(wait);
}
int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  std::ifstream inputs(argv[1]);
  KJ_REQUIRE(inputs.good());
  std::string line;
  while (std::getline(inputs, line)) {
    std::istringstream events(line);
    bool wire; events >> wire;
    State state;
    unsigned calls=0, done=0, failed=0, returned=0, observed=0;
    Harness::Client client(kj::heap<Controlled>(state));
    auto pipe=kj::newTwoWayPipe();
    capnp::TwoPartyClient hosted(*pipe.ends[0], client, capnp::rpc::twoparty::Side::SERVER);
    capnp::TwoPartyClient caller(*pipe.ends[1]);
    if (wire) client=caller.bootstrap().castAs<Harness>();
    auto offered=kj::newPromiseAndFulfiller<Harness::Client>();
    Harness::Client target(kj::mv(offered.promise));
    auto call=client.pendingRequest().send();
    Harness::PendingResults::Pipeline pipeline(kj::mv(call));
    kj::String parentError;
    auto parent=call.then([&](auto&&) { returned=1; },
        [&](kj::Exception&& e) { returned=2; parentError=kj::str(e); }).eagerlyEvaluate(nullptr);
    std::vector<kj::Promise<void>> children;
    std::optional<kj::Promise<void>> observer;
    drain(wait);
    parent.poll(wait);
    KJ_REQUIRE(state.context.has_value(),wire,returned,parentError);
    unsigned event;
    while (events >> event) {
      switch (event) {
        case 1:
        case 7: {
          capnp::PipelineBuilder<Harness::PendingResults> builder;
          if (event == 1) builder.setCap(target);
          else builder.setCap(Harness::Client(nullptr));
          // C++ ignores a second publication; Rust reports an error. Compare
          // the shared contract: the first published routing stays in effect.
          state.context->setPipeline(builder.build());
          break;
        }
        case 2: offered.fulfiller->fulfill(Harness::Client(kj::heap<Echo>(calls))); break;
        case 3: offered.fulfiller->reject(KJ_EXCEPTION(FAILED,"target rejected")); break;
        case 4:
        case 5:
          if (event == 4) state.context->getResults().setCap(target);
          state.context.reset();
          state.gate->fulfill(event == 4);
          break;
        case 6:
          children.push_back(pipeline.getCap().echoRequest().send().then([&](auto&& r) {
            KJ_REQUIRE(r.getValue() == 73);
            ++done;
          }, [&](kj::Exception&&) { ++failed; }).eagerlyEvaluate(nullptr));
          break;
        case 8:
          observer.emplace(pipeline.getCap().whenResolved().then([&]() { observed=1; },
              [&](kj::Exception&&) { observed=2; }).eagerlyEvaluate(nullptr));
          break;
        default: KJ_FAIL_REQUIRE("unknown event",event);
      }
      drain(wait);
      parent.poll(wait);
      for (auto& child: children) child.poll(wait);
      if (observer) observer->poll(wait);
      std::cout << done << ' ' << failed << ' ' << returned << ' ' << calls << ' ' << observed << '\n';
    }
  }
}
