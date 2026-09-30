#include <capnp/rpc-twoparty.h>
#include <capnp/serialize-async.h>
#include <kj/async.h>
#include <kj/debug.h>
#include <kj/time.h>
#include <fstream>
#include <iostream>
#include <string>

class Clock final: public kj::MonotonicClock {
public:
  uint64_t seconds = 0;
  kj::TimePoint now() const override {
    return kj::origin<kj::TimePoint>() + seconds * kj::SECONDS;
  }
};
class Stream final: public capnp::MessageStream {
public:
  size_t batches = 0;
  kj::Own<kj::PromiseFulfiller<void>> pending;
  kj::Promise<kj::Maybe<capnp::MessageReaderAndFds>> tryReadMessage(
      kj::ArrayPtr<kj::OwnFd>, capnp::ReaderOptions, kj::ArrayPtr<capnp::word>) override {
    return kj::NEVER_DONE;
  }
  kj::Promise<void> writeMessage(kj::ArrayPtr<const int>,
      kj::ArrayPtr<const kj::ArrayPtr<const capnp::word>>) override {
    KJ_FAIL_REQUIRE("expected a batch without file descriptors");
  }
  kj::Promise<void> writeMessages(
      kj::ArrayPtr<kj::ArrayPtr<const kj::ArrayPtr<const capnp::word>>> messages) override {
    KJ_REQUIRE(pending.get() == nullptr);
    KJ_REQUIRE(messages.size() > 0);
    ++batches;
    auto pair = kj::newPromiseAndFulfiller<void>();
    pending = kj::mv(pair.fulfiller);
    return kj::mv(pair.promise);
  }
  kj::Maybe<int> getSendBufferSize() override { return kj::none; }
  kj::Promise<void> end() override { return kj::READY_NOW; }
  void complete() { auto owned = kj::mv(pending); KJ_REQUIRE(owned.get() != nullptr); owned->fulfill(); }
};
int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  std::ifstream input(argv[1]);
  KJ_REQUIRE(input.good());
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  Clock clock;
  Stream stream;
  capnp::TwoPartyVatNetwork network(stream, capnp::rpc::twoparty::Side::CLIENT, {}, clock);
  capnp::MallocMessageBuilder host;
  auto id = host.initRoot<capnp::rpc::twoparty::VatId>();
  id.setSide(capnp::rpc::twoparty::Side::SERVER);
  auto connection = KJ_ASSERT_NONNULL(network.connect(id));
  connection->setIdle(false);
  std::string op;
  while (input >> op) {
    if (op == "time") { input >> clock.seconds; }
    else if (op == "send") {
      uint size; input >> size;
      auto message = connection->newOutgoingMessage(64);
      auto data = message->getBody().initAs<capnp::Data>(size);
      for (auto& byte: data) byte = static_cast<kj::byte>(size);
      message->send();
    } else if (op == "poll") {
      kj::Promise<void>(kj::NEVER_DONE).poll(wait);
    } else if (op == "complete") {
      stream.complete();
      kj::Promise<void>(kj::NEVER_DONE).poll(wait);
    } else { KJ_FAIL_REQUIRE("unknown operation", op.c_str()); }
    std::cout << network.getCurrentQueueCount() << ' ' << network.getCurrentQueueSize()
              << ' ' << network.getOutgoingMessageWaitTime() / kj::SECONDS
              << ' ' << stream.batches << '\n';
  }
}
