// Pinned C++ facade calls under the same bounded lifecycle traces as Rust.
#include "runtime-test.capnp.h"
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <memory>
#include <optional>
#include <sstream>

struct Echo final: Harness::Server {
  explicit Echo(unsigned& calls): calls(calls) {}
  unsigned& calls;
  kj::Promise<void> echo(EchoContext c) override {
    ++calls;
    c.getResults().setValue(73);
    return kj::READY_NOW;
  }
};
struct DropCounter {
  explicit DropCounter(unsigned& drops): drops(drops) {}
  unsigned& drops;
  ~DropCounter() { ++drops; }
};
void settle(kj::WaitScope& wait) {
  wait.poll();
}
int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  std::ifstream input(argv[1]);
  KJ_REQUIRE(input.good());
  auto io = kj::setupAsyncIo();
  auto& wait = io.waitScope;
  std::string line;
  while (std::getline(input, line)) {
    unsigned ownedDrops=0, borrowedDrops=0, calls=0, drained=0;
    auto server = std::make_unique<capnp::TwoPartyServer>(kj::heap<Echo>(calls));
    kj::Own<kj::AsyncIoStream> peerIo[2];
    kj::Own<kj::AsyncIoStream> borrowedIo;
    std::unique_ptr<capnp::TwoPartyClient> peers[2];
    std::optional<Harness::Client> caps[2];
    std::optional<kj::Promise<void>> borrowed;
    std::optional<kj::Promise<void>> drain;
    std::istringstream events(line);
    unsigned event;
    while (events >> event) {
      switch (event) {
        case 1: {
          auto pipe = io.provider->newTwoWayPipe();
          server->accept(kj::mv(pipe.ends[0]).attach(kj::heap<DropCounter>(ownedDrops)));
          peerIo[0] = kj::mv(pipe.ends[1]);
          peers[0] = std::make_unique<capnp::TwoPartyClient>(*peerIo[0]);
          caps[0].emplace(peers[0]->bootstrap().castAs<Harness>());
          break;
        }
        case 2: {
          auto pipe = io.provider->newTwoWayPipe();
          borrowedIo = kj::mv(pipe.ends[0]).attach(kj::heap<DropCounter>(borrowedDrops));
          borrowed.emplace(server->accept(*borrowedIo));
          peerIo[1] = kj::mv(pipe.ends[1]);
          peers[1] = std::make_unique<capnp::TwoPartyClient>(*peerIo[1]);
          caps[1].emplace(peers[1]->bootstrap().castAs<Harness>());
          break;
        }
        case 3: drain.emplace(server->drain()); drained=1; break;
        case 4:
        case 9: {
          unsigned i = event == 9 ? 1 : 0;
          peerIo[i]->shutdownWrite();
          peers[i].reset();
          peerIo[i] = nullptr;
          break;
        }
        case 5: borrowed.reset(); break;
        case 6:
        case 7: {
          auto response = caps[event-6]->echoRequest().send().wait(wait);
          KJ_REQUIRE(response.getValue() == 73);
          break;
        }
        case 8: server.reset(); break;
        default: KJ_FAIL_REQUIRE("unknown event", event);
      }
      settle(wait);
      if (borrowed && borrowed->poll(wait)) {
        borrowed->wait(wait);
        borrowed.reset();
      }
      if (drain && drain->poll(wait)) {
        try { drain->wait(wait); drained=2; }
        catch (const kj::Exception&) { drained=3; }
        drain.reset();
      }
      std::cout << ownedDrops << ' ' << borrowedDrops << ' ' << drained << ' ' << calls
                << ' ' << unsigned(borrowed.has_value()) << ',';
    }
    std::cout << '\n';
  }
}
