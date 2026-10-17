#include "echo.capnp.h"
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <kj/timer.h>
#include <array>
#include <chrono>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

namespace {
constexpr std::array<capnp::byte, 32> PAYLOAD = {
  0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
  16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
};
uint64_t traverse(AccessNode::Reader node, uint16_t depth, uint64_t sequence) {
  KJ_REQUIRE(depth >= 1 && depth <= 32);
  uint64_t sum = 0;
  for (uint16_t level = 0; level < depth; ++level) {
    auto value = sequence + level;
    auto payload = node.getPayload();
    KJ_REQUIRE(node.getValue() == value && payload.size() == PAYLOAD.size());
    KJ_REQUIRE(std::memcmp(payload.begin(), PAYLOAD.data(), PAYLOAD.size()) == 0);
    KJ_REQUIRE(node.hasNext() == (level + 1 < depth));
    sum += value;
    for (auto byte: payload) sum += byte;
    if (level + 1 < depth) node = node.getNext();
  }
  return sum;
}
void fill(AccessNode::Builder node, uint16_t depth, uint64_t sequence) {
  for (uint16_t level = 0; level < depth; ++level) {
    node.setValue(sequence + level);
    node.setPayload(kj::arrayPtr(PAYLOAD.data(), PAYLOAD.size()));
    if (level + 1 < depth) node = node.initNext();
  }
}
class Service final: public AccessService::Server {
  kj::Maybe<AccessService::Client> next;
  uint16_t hops;
public:
  Service(kj::Maybe<AccessService::Client> next, uint16_t hops): next(kj::mv(next)), hops(hops) {}
  kj::Promise<void> describe(DescribeContext context) override {
    context.getResults().setHops(hops);
    return kj::READY_NOW;
  }
  kj::Promise<void> access(AccessContext context) override {
    auto params = context.getParams();
    auto root = params.getRoot();
    auto depth = params.getDepth();
    auto sum = traverse(root, depth, params.getSequence());
    KJ_IF_SOME(client, next) {
      auto request = client.accessRequest();
      request.setSequence(params.getSequence());
      request.setDepth(depth);
      request.setRoot(root);
      return request.send().then([context = kj::mv(context), depth, sum](auto response) mutable {
        auto results = context.getResults();
        results.setChecksum(sum + response.getChecksum());
        results.setVisits(depth + response.getVisits());
        results.setRoot(response.getRoot());
      });
    }
    auto results = context.getResults();
    results.setChecksum(sum);
    results.setVisits(depth);
    results.setRoot(root);
    return kj::READY_NOW;
  }
};
struct Connection {
  kj::Own<kj::AsyncIoStream> stream;
  capnp::TwoPartyClient rpc;
  AccessService::Client client;
  explicit Connection(kj::Own<kj::AsyncIoStream> stream)
    : stream(kj::mv(stream)), rpc(*this->stream), client(rpc.bootstrap().castAs<AccessService>()) {}
};
struct Case { const char* name; uint16_t depth, width, connections, hops; };
constexpr Case CASES[] = {
  {"deep-8", 8, 1, 1, 1},
  {"deep-32", 32, 1, 1, 1},
  {"concurrent-32", 1, 32, 1, 1},
  {"deep-32-concurrent-32", 32, 32, 1, 1},
  {"services-4-deep-16", 16, 1, 1, 4},
  {"services-4-deep-16-concurrent-32", 16, 32, 1, 4},
  {"services-4-deep-16-clients-8", 16, 32, 8, 4},
  {"services-8-deep-16-clients-8", 16, 32, 8, 8}
};
kj::Promise<void> access(AccessService::Client& client, const Case& c, uint64_t sequence) {
  auto request = client.accessRequest();
  request.setSequence(sequence);
  request.setDepth(c.depth);
  fill(request.initRoot(), c.depth, sequence);
  return request.send().then([&c, sequence](auto response) {
    auto sum = traverse(response.getRoot(), c.depth, sequence);
    KJ_REQUIRE(response.getChecksum() == sum * c.hops);
    KJ_REQUIRE(response.getVisits() == uint32_t(c.depth) * c.hops);
  });
}
}

int runTopologies(int argc, char** argv) {
  auto io = kj::setupAsyncIo();
  auto connect = [&](const char* address) {
    auto parsed = io.provider->getNetwork().parseAddress(address).wait(io.waitScope);
    return kj::heap<Connection>(io.provider->getTimer().timeoutAfter(
        10 * kj::SECONDS, parsed->connect()).wait(io.waitScope));
  };
  if ((argc == 3 || argc == 4) && std::string(argv[1]) == "topology-server") {
    kj::Own<Connection> downstream;
    kj::Maybe<AccessService::Client> next;
    uint16_t hops = 1;
    if (argc == 4) {
      downstream = connect(argv[3]);
      auto response = io.provider->getTimer().timeoutAfter(
          10 * kj::SECONDS, downstream->client.describeRequest().send()).wait(io.waitScope);
      KJ_REQUIRE(response.getHops() >= 1 && response.getHops() < 8);
      hops += response.getHops();
      next = downstream->client;
    }
    auto address = io.provider->getNetwork().parseAddress(argv[2]).wait(io.waitScope);
    auto listener = address->listen();
    capnp::TwoPartyServer server(kj::heap<Service>(kj::mv(next), hops));
    std::cout << "{\"port\":" << listener->getPort() << ",\"hops\":" << hops << "}" << std::endl;
    server.listen(*listener).wait(io.waitScope);
    return 0;
  }
  KJ_REQUIRE(argc == 6 && std::string(argv[1]) == "topology-measure");
  const Case* selected = nullptr;
  for (auto& c: CASES) if (c.name == std::string(argv[3])) selected = &c;
  KJ_REQUIRE(selected != nullptr, "unknown topology");
  const auto& c = *selected;
  auto warmup = std::stoull(argv[4]);
  auto iterations = std::stoull(argv[5]);
  KJ_REQUIRE(warmup <= 100000 && iterations > 0 && iterations <= 100000);
  std::vector<kj::Own<Connection>> clients;
  for (uint16_t n = 0; n < c.connections; ++n) {
    auto connection = connect(argv[2]);
    auto response = io.provider->getTimer().timeoutAfter(
        10 * kj::SECONDS, connection->client.describeRequest().send()).wait(io.waitScope);
    KJ_REQUIRE(response.getHops() == c.hops, "wrong service topology");
    clients.push_back(kj::mv(connection));
  }
  using Clock = std::chrono::steady_clock;
  Clock::time_point start;
  std::vector<uint64_t> samples;
  samples.reserve(iterations);
  for (uint64_t sequence = 0; sequence < warmup + iterations; ++sequence) {
    if (sequence == warmup) start = Clock::now();
    auto before = Clock::now();
    {
      auto calls = kj::heapArrayBuilder<kj::Promise<void>>(c.width);
      for (uint16_t lane = 0; lane < c.width; ++lane) {
        calls.add(access(clients[lane % c.connections]->client, c, sequence * c.width + lane));
      }
      io.provider->getTimer().timeoutAfter(10 * kj::SECONDS, kj::joinPromises(calls.finish())).wait(io.waitScope);
    }
    if (sequence >= warmup) samples.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - before).count());
  }
  auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - start).count();
  std::cout << "{\"topology_version\":1,\"implementation\":\"capnp-cpp\",\"case\":{\"name\":\""
      << c.name << "\",\"depth\":" << c.depth << ",\"width\":" << c.width
      << ",\"connections\":" << c.connections << ",\"hops\":" << c.hops
      << ",\"payload_bytes_per_node\":32},\"warmup\":" << warmup << ",\"iterations\":"
      << iterations << ",\"elapsed_ns\":" << elapsed << ",\"latency_ns\":[";
  for (size_t n = 0; n < samples.size(); ++n) { if (n) std::cout << ','; std::cout << samples[n]; }
  std::cout << "]}" << std::endl;
  return 0;
}
