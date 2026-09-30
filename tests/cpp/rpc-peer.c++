#include "runtime-test.capnp.h"
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <iostream>
#include <string>

class Echo final: public Harness::Server {
  kj::Promise<void> echo(EchoContext context) override {
    if (context.getParams().getValue() == 1001) return KJ_EXCEPTION(OVERLOADED, "cpp failure");
    context.getResults().setValue(100 + context.getParams().getValue());
    return kj::READY_NOW;
  }
  kj::Promise<void> bounce(BounceContext context) override {
    context.getResults().setCap(context.getParams().getCap());
    return kj::READY_NOW;
  }
  kj::Promise<void> pending(PendingContext context) override {
    context.getResults().setCap(kj::heap<Echo>());
    return kj::READY_NOW;
  }
  kj::Promise<void> tail(TailContext context) override {
    auto params = context.getParams();
    auto request = params.getCap().echoRequest();
    request.setValue(params.getValue());
    return context.tailCall(kj::mv(request));
  }
  kj::Promise<void> tailRoundtrip(TailRoundtripContext context) override {
    auto params = context.getParams();
    if (params.getValue() == 1000) {
      auto request = params.getCap().bounceRequest();
      request.setCap(thisCap().castAs<Harness>());
      auto pipeline = request.sendForPipeline();
      auto child = pipeline.getCap().echoRequest();
      child.setValue(5);
      return child.send().then([context = kj::mv(context)](auto response) mutable {
        context.getResults().setValue(response.getValue());
      });
    }
    if (params.getValue() == 999) {
      auto check = params.getCap().echoRequest();
      check.setValue(999);
      return check.send().then([](auto) -> kj::Promise<void> {
        KJ_FAIL_REQUIRE("expected Rust exception");
      }, [context = kj::mv(context)](kj::Exception&& error) mutable {
        KJ_REQUIRE(error.getType() == kj::Exception::Type::OVERLOADED);
        KJ_REQUIRE(std::string(error.getRemoteTrace().cStr()).find("rust-trace:callback failed") != std::string::npos);
        context.getResults().setValue(999);
      });
    }
    auto request = params.getCap().tailRequest();
    request.setCap(thisCap().castAs<Harness>());
    request.setValue(params.getValue());
    return request.send().then([context = kj::mv(context)](auto response) mutable {
      context.getResults().setValue(response.getValue());
    });
  }

};
int main() {
  auto io = kj::setupAsyncIo();
  auto address = io.provider->getNetwork().parseAddress("127.0.0.1", 0).wait(io.waitScope);
  auto listener = address->listen();
  capnp::TwoPartyServer server(kj::heap<Echo>(), kj::Function<kj::String(const kj::Exception&)>([](const kj::Exception& error) {
    return kj::str("cpp-trace:", error.getDescription());
  }));
  std::cout << listener->getPort() << std::endl;
  server.listen(*listener).wait(io.waitScope);
}
