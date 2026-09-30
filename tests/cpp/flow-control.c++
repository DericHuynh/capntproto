// Exercise the pinned C++ controllers through the same timed operations as Rust.
#include <capnp/rpc.h>
#include <kj/async.h>
#include <kj/debug.h>
#include <kj/time.h>
#include <fstream>
#include <iostream>
#include <memory>
#include <vector>

class Clock final: public kj::MonotonicClock {
public:
  uint64_t micros = 0;
  kj::TimePoint now() const override {
    return kj::origin<kj::TimePoint>() + micros * kj::MICROSECONDS;
  }
};

class Window final: public capnp::RpcFlowController::WindowGetter {
public:
  size_t bytes = 0;
  size_t getWindow() override { return bytes; }
};

class Message final: public capnp::OutgoingRpcMessage {
public:
  Message(size_t words, size_t& sent): words(words), sent(sent) {}
  capnp::AnyPointer::Builder getBody() override { KJ_FAIL_REQUIRE("unused body"); }
  size_t sizeInWords() override { return words; }
  void send() override { ++sent; }
private:
  size_t words;
  size_t& sent;
};

int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  std::ifstream input(argv[1]);
  KJ_REQUIRE(input.good());
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  Clock clock;
  Window window;
  size_t sent = 0;
  kj::Own<capnp::RpcFlowController> controller;
  std::vector<kj::Own<kj::PromiseFulfiller<void>>> acks;
  std::vector<std::unique_ptr<kj::Promise<void>>> credits;
  std::vector<int> results;
  std::string op;
  while (input >> op) {
    uint64_t a, b;
    input >> a;
    if (op == "new") {
      input >> b;
      credits.clear();
      controller = nullptr;
      acks.clear();
      results.clear();
      sent = 0;
      clock.micros = 0;
      window.bytes = b;
      controller = a == 0
          ? capnp::RpcFlowController::newVariableWindowController(window)
          : capnp::RpcFlowController::newAdaptiveController(b, clock);
    } else if (op == "time") {
      clock.micros = a;
    } else if (op == "window") {
      window.bytes = a;
    } else if (op == "send") {
      auto paf = kj::newPromiseAndFulfiller<void>();
      credits.emplace_back(new kj::Promise<void>(controller->send(
          kj::heap<Message>(a, sent), kj::mv(paf.promise))));
      acks.push_back(kj::mv(paf.fulfiller));
      results.push_back(2);
      KJ_REQUIRE(sent == acks.size(), "send must be immediate");
    } else if (op == "ack") {
      input >> b;
      if (b == 0) {
        acks[a]->fulfill();
      } else {
        acks[a]->reject(KJ_EXCEPTION(DISCONNECTED, "original ack failure"));
      }
      acks[a] = nullptr;
    } else {
      KJ_FAIL_REQUIRE("unknown operation", op.c_str());
    }
    wait.poll();
    for (size_t i = 0; i < credits.size(); ++i) {
      if (credits[i] && credits[i]->poll(wait)) {
        try {
          credits[i]->wait(wait);
          results[i] = 1;
        } catch (const kj::Exception&) {
          results[i] = 3;
        }
        credits[i].reset();
      }
    }
    std::cout << sent << ':';
    for (int result: results) std::cout << result;
    std::cout << '\n';
  }
}
