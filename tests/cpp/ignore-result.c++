#include "runtime-test.capnp.h"
#include "cancellation-policy.capnp.h"
#include <capnp/capability.h>
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <memory>
#include <optional>
#include <sstream>

struct Held {
  virtual ~Held() = default;
  virtual void publish() = 0;
};
template <typename P, typename R>
struct Context final: Held {
  capnp::CallContext<P, R> context;
  explicit Context(capnp::CallContext<P, R> c): context(kj::mv(c)) {}
  void publish() override {
    capnp::PipelineBuilder<R> pipeline;
    pipeline.setCap(context.getResults().getCap());
    context.setPipeline(pipeline.build());
  }
};
struct State {
  std::unique_ptr<Held> context;
  kj::Own<kj::PromiseFulfiller<bool>> gate;
  bool running=false, alive=false;
  unsigned completed=0;
};
struct Value final: Harness::Server {
  State& state;
  explicit Value(State& s): state(s) { state.alive=true; }
  ~Value() { state.alive=false; }
};
struct Running {
  State& state;
  explicit Running(State& s): state(s) { state.running=true; }
  ~Running() { state.running=false; state.context.reset(); }
};
struct Service final: Policy::Server {
  State& state;
  explicit Service(State& s): state(s) {}
  template <typename P, typename R>
  kj::Promise<void> work(capnp::CallContext<P, R> c) {
    c.getResults().setCap(kj::heap<Value>(state));
    state.context=std::make_unique<Context<P,R>>(kj::mv(c));
    auto gate=kj::newPromiseAndFulfiller<bool>();
    state.gate=kj::mv(gate.fulfiller);
    return gate.promise.then([this](bool ok) {
      state.completed=ok?1:2;
      KJ_REQUIRE(ok,"late failure");
    }).attach(kj::heap<Running>(state));
  }
  kj::Promise<void> pending(PendingContext c) override { return work(kj::mv(c)); }
  kj::Promise<void> cancellable(CancellableContext c) override { return work(kj::mv(c)); }
};
void drain(kj::WaitScope& wait) {
  // evalLater loops can starve the transport's evalLast write batching and
  // cancellation cleanup. Drain the event loop, including those callbacks.
  wait.poll();
}
int main(int argc, char** argv) {
  KJ_REQUIRE(argc==2);
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  std::ifstream inputs(argv[1]);
  KJ_REQUIRE(inputs.good());
  std::string line;
  while (std::getline(inputs,line)) {
    std::istringstream events(line);
    bool wire; events >> wire;
    State state;
    Policy::Client client(kj::heap<Service>(state));
    auto pipe=kj::newTwoWayPipe();
    capnp::TwoPartyClient hosted(*pipe.ends[0],client,capnp::rpc::twoparty::Side::SERVER);
    capnp::TwoPartyClient caller(*pipe.ends[1]);
    if (wire) client=caller.bootstrap().castAs<Policy>();
    std::optional<kj::Promise<void>> promise;
    unsigned outcome=0, event;
    while (events >> event) {
      switch (event) {
        case 1:
        case 2: {
          auto call=event==1?client.pendingRequest().sendIgnoringResult():
                            client.cancellableRequest().sendIgnoringResult();
          promise.emplace(call.then([&]() { outcome=1; }, [&](kj::Exception&& e) {
            KJ_REQUIRE(e.getDescription().contains("late failure"));
            outcome=2;
          }).eagerlyEvaluate(nullptr));
          break;
        }
        case 3: state.context->publish(); break;
        case 4: state.context.reset(); break;
        case 5:
        case 6: state.gate->fulfill(event==5); break;
        case 7: promise.reset(); break;
        default: KJ_FAIL_REQUIRE("unknown event",event);
      }
      drain(wait);
      if (promise) promise->poll(wait);
      if (outcome) promise.reset();
      drain(wait);
      std::cout << promise.has_value() << ' ' << state.running << ' ' << state.completed
                << ' ' << outcome << ' ' << state.alive << ' ' << bool(state.context) << '\n';
    }
    promise.reset();
    drain(wait);
    if (state.running) { state.gate->fulfill(true); drain(wait); }
    KJ_REQUIRE(!state.running && !state.alive,line.c_str(),state.running,state.alive,state.completed);
  }
}
