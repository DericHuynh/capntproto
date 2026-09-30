#include "echo.capnp.h"
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <chrono>
#include <iostream>
#include <string>
#include <vector>

class Service final: public Echo::Server {
  kj::Promise<void> echo(EchoContext context) override {
    auto params = context.getParams();
    auto results = context.getResults();
    results.setSequence(params.getSequence());
    results.setPayload(params.getPayload());
    return kj::READY_NOW;
  }
};
int main(int argc, char** argv) {
  auto io = kj::setupAsyncIo();
  if (argc == 2 && std::string(argv[1]) == "server") {
    auto address = io.provider->getNetwork().parseAddress("127.0.0.1", 0).wait(io.waitScope);
    auto listener = address->listen();
    capnp::TwoPartyServer server(kj::heap<Service>());
    std::cout << "{\"address\":\"127.0.0.1:" << listener->getPort() << "\",\"public\":null}" << std::endl;
    server.listen(*listener).wait(io.waitScope);
  } else {
    KJ_REQUIRE(argc == 6 && std::string(argv[1]) == "measure");
    auto bytes = std::stoull(argv[3]);
    auto warmup = std::stoull(argv[4]);
    auto iterations = std::stoull(argv[5]);
    KJ_REQUIRE(bytes <= 1024 * 1024 && warmup <= 100000 && iterations > 0 && iterations <= 1000000);
    std::vector<capnp::byte> payload(bytes);
    for (size_t n = 0; n < bytes; ++n) payload[n] = n % 251;
    auto address = io.provider->getNetwork().parseAddress(argv[2]).wait(io.waitScope);
    auto stream = address->connect().wait(io.waitScope);
    capnp::TwoPartyClient client(*stream);
    auto echo = client.bootstrap().castAs<Echo>();
    std::vector<uint64_t> samples;
    samples.reserve(iterations);
    using Clock = std::chrono::steady_clock;
    Clock::time_point start;
    for (uint64_t sequence = 0; sequence < warmup + iterations; ++sequence) {
      if (sequence == warmup) start = Clock::now();
      auto before = Clock::now();
      auto request = echo.echoRequest();
      request.setSequence(sequence);
      request.setPayload(kj::arrayPtr(payload.data(), payload.size()));
      auto response = request.send().wait(io.waitScope);
      auto returned = response.getPayload();
      KJ_REQUIRE(response.getSequence() == sequence && returned.size() == bytes);
      for (size_t n = 0; n < bytes; ++n) KJ_REQUIRE(returned[n] == payload[n]);
      if (sequence >= warmup) samples.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - before).count());
    }
    auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - start).count();
    std::cout << "{\"protocol\":\"capnp-cpp\",\"payload_bytes\":" << bytes << ",\"warmup\":" << warmup << ",\"iterations\":" << iterations << ",\"elapsed_ns\":" << elapsed << ",\"latency_ns\":[";
    for (size_t n = 0; n < samples.size(); ++n) { if (n) std::cout << ','; std::cout << samples[n]; }
    std::cout << "]}" << std::endl;
  }
}
