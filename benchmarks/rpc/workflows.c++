#include "echo.capnp.h"
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <kj/timer.h>
#include <chrono>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

namespace {
class State final: public Cell::Server {
  uint64_t version = 0;
  std::vector<capnp::byte> payload;
public:
  explicit State(capnp::Data::Reader data): payload(data.begin(), data.end()) {}
  kj::Promise<void> read(ReadContext context) override {
    auto results = context.getResults();
    results.setVersion(version);
    results.setPayload(kj::arrayPtr(payload.data(), payload.size()));
    return kj::READY_NOW;
  }
  kj::Promise<void> write(WriteContext context) override {
    auto params = context.getParams();
    KJ_REQUIRE(params.getExpectedVersion() == version && version < UINT64_MAX);
    ++version;
    auto data = params.getPayload();
    payload.assign(data.begin(), data.end());
    context.getResults().setVersion(version);
    return kj::READY_NOW;
  }
};
class Service final: public Workflows::Server {
  kj::Promise<void> echo(EchoContext context) override {
    auto params = context.getParams();
    auto results = context.getResults();
    results.setSequence(params.getSequence());
    results.setPayload(params.getPayload());
    return kj::READY_NOW;
  }
  kj::Promise<void> open(OpenContext context) override {
    context.getResults().setCell(kj::heap<State>(context.getParams().getPayload()));
    return kj::READY_NOW;
  }
  kj::Promise<void> callback(CallbackContext context) override {
    auto params = context.getParams();
    auto request = params.getSink().notifyRequest();
    request.setSequence(params.getSequence());
    request.setPayload(params.getPayload());
    return request.send().then([context = kj::mv(context)](auto response) mutable {
      context.getResults().setSequence(response.getSequence());
    });
  }
};
class Sink final: public Callback::Server {
  std::vector<capnp::byte> payload;
  uint64_t& calls;
public:
  Sink(const std::vector<capnp::byte>& data, uint64_t& calls): payload(data), calls(calls) {}
  kj::Promise<void> notify(NotifyContext context) override {
    auto params = context.getParams();
    auto data = params.getPayload();
    KJ_REQUIRE(data.size() == payload.size());
    KJ_REQUIRE(payload.empty() || std::memcmp(data.begin(), payload.data(), payload.size()) == 0);
    KJ_REQUIRE(params.getSequence() == calls);
    ++calls;
    context.getResults().setSequence(params.getSequence());
    return kj::READY_NOW;
  }
};
kj::Promise<void> echo(Workflows::Client& client, uint64_t sequence, const std::vector<capnp::byte>& payload) {
  auto request = client.echoRequest();
  request.setSequence(sequence);
  request.setPayload(kj::arrayPtr(payload.data(), payload.size()));
  return request.send().then([sequence, &payload](auto response) {
    auto data = response.getPayload();
    KJ_REQUIRE(response.getSequence() == sequence && data.size() == payload.size());
    KJ_REQUIRE(payload.empty() || std::memcmp(data.begin(), payload.data(), payload.size()) == 0);
  });
}
kj::Promise<void> read(Cell::Client client, uint64_t version, const std::vector<capnp::byte>& payload) {
  return client.readRequest().send().then([version, &payload](auto response) {
    auto data = response.getPayload();
    KJ_REQUIRE(response.getVersion() == version && data.size() == payload.size());
    KJ_REQUIRE(payload.empty() || std::memcmp(data.begin(), payload.data(), payload.size()) == 0);
  });
}
kj::Promise<void> operation(Workflows::Client& client, Cell::Client& state, Callback::Client& sink, uint64_t& callbacks,
                          const std::string& name, uint64_t sequence, const std::vector<capnp::byte>& payload) {
  if (name == "fanout-32") {
    auto calls = kj::heapArrayBuilder<kj::Promise<void>>(32);
    for (uint64_t i = 0; i < 32; ++i) calls.add(echo(client, sequence * 32 + i, payload));
    return kj::joinPromises(calls.finish());
  }
  if (name == "dependent-read" || name == "pipelined-read") {
    auto request = client.openRequest();
    request.setPayload(kj::arrayPtr(payload.data(), payload.size()));
    auto opened = request.send();
    if (name == "pipelined-read") {
      auto reading = read(opened.getCell(), 0, payload);
      return reading.then([opened = kj::mv(opened)]() mutable {
        return opened.then([](auto response) { response.getCell(); });
      });
    }
    return opened.then([&payload](auto response) { return read(response.getCell(), 0, payload); });
  }
  if (name == "write-read") {
    auto request = state.writeRequest();
    request.setExpectedVersion(sequence);
    request.setPayload(kj::arrayPtr(payload.data(), payload.size()));
    return request.send().then([&state, sequence, &payload](auto response) {
      KJ_REQUIRE(response.getVersion() == sequence + 1);
      return read(state, sequence + 1, payload);
    });
  }
  if (name == "callback") {
    auto request = client.callbackRequest();
    request.setSink(sink);
    request.setSequence(sequence);
    request.setPayload(kj::arrayPtr(payload.data(), payload.size()));
    return request.send().then([sequence, &callbacks](auto response) { KJ_REQUIRE(response.getSequence() == sequence && callbacks == sequence + 1); });
  }
  return echo(client, sequence, payload);
}
}

int runWorkflows(int argc, char** argv) {
  auto io = kj::setupAsyncIo();
  if (argc == 3 && std::string(argv[1]) == "workflow-server") {
    auto address = io.provider->getNetwork().parseAddress(argv[2]).wait(io.waitScope);
    auto listener = address->listen();
    capnp::TwoPartyServer server(kj::heap<Service>());
    std::cout << "{\"ready\":true}" << std::endl;
    server.listen(*listener).wait(io.waitScope);
    return 0;
  }
  KJ_REQUIRE(argc == 6 && std::string(argv[1]) == "workflow-measure");
  std::string name(argv[3]);
  size_t bytes = 1024;
  if (name == "unary-empty") bytes = 0;
  else if (name == "unary-small" || name == "callback") bytes = 64;
  else if (name == "bulk-64k") bytes = 65536;
  else if (name == "bulk-256k") bytes = 262144;
  else if (name == "bulk-1m") bytes = 1048576;
  else if (name == "bulk-4m") bytes = 4194304;
  else KJ_REQUIRE(name == "unary-1k" || name == "fanout-32" || name == "dependent-read" || name == "pipelined-read" || name == "write-read");
  auto warmup = std::stoull(argv[4]);
  auto iterations = std::stoull(argv[5]);
  KJ_REQUIRE(warmup <= 100000 && iterations > 0 && iterations <= 100000);
  std::vector<capnp::byte> payload(bytes);
  for (size_t n = 0; n < bytes; ++n) payload[n] = n % 251;
  auto address = io.provider->getNetwork().parseAddress(argv[2]).wait(io.waitScope);
  auto stream = address->connect().wait(io.waitScope);
  capnp::TwoPartyClient rpc(*stream);
  auto client = rpc.bootstrap().castAs<Workflows>();
  auto request = client.openRequest();
  request.setPayload(kj::arrayPtr(payload.data(), payload.size()));
  auto state = io.provider->getTimer().timeoutAfter(10 * kj::SECONDS, request.send()).wait(io.waitScope).getCell();
  uint64_t callbacks = 0;
  Callback::Client sink(kj::heap<Sink>(payload, callbacks));
  std::vector<uint64_t> samples;
  samples.reserve(iterations);
  using Clock = std::chrono::steady_clock;
  Clock::time_point start;
  for (uint64_t sequence = 0; sequence < warmup + iterations; ++sequence) {
    if (sequence == warmup) start = Clock::now();
    auto before = Clock::now();
    io.provider->getTimer().timeoutAfter(10 * kj::SECONDS,
        operation(client, state, sink, callbacks, name, sequence, payload)).wait(io.waitScope);
    if (sequence >= warmup) samples.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - before).count());
  }
  auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - start).count();
  auto width = name == "fanout-32" ? 32 : 1;
  auto rpcs = width == 32 ? 32 : (name == "dependent-read" || name == "pipelined-read" || name == "write-read" || name == "callback" ? 2 : 1);
  std::cout << "{\"workflow_version\":3,\"implementation\":\"capnp-cpp\",\"case\":{\"name\":\"" << name << "\",\"payload_bytes\":" << bytes << ",\"width\":" << width << ",\"rpcs_per_iteration\":" << rpcs << "},\"warmup\":" << warmup << ",\"iterations\":" << iterations << ",\"elapsed_ns\":" << elapsed << ",\"latency_ns\":[";
  for (size_t n = 0; n < samples.size(); ++n) { if (n) std::cout << ','; std::cout << samples[n]; }
  std::cout << "]}" << std::endl;
  return 0;
}
