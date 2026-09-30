// Replay live hint changes through the actual pinned TwoPartyVatNetwork,
// including its shared unavailable-query cache. Message IO is not simulated.
#include <capnp/rpc-twoparty.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <memory>
#include <vector>

class Stream final: public capnp::MessageStream {
public:
  unsigned hint = 2;
  unsigned queries = 0;
  kj::Maybe<int> getSendBufferSize() override {
    ++queries;
    if (hint == 0) return kj::none;
    return hint == 1 ? 0 : hint == 2 ? 16384 : 65536;
  }
  kj::Promise<kj::Maybe<capnp::MessageReaderAndFds>> tryReadMessage(
      kj::ArrayPtr<kj::OwnFd>, capnp::ReaderOptions, kj::ArrayPtr<capnp::word>) override {
    return kj::NEVER_DONE;
  }
  kj::Promise<void> writeMessage(kj::ArrayPtr<const int>,
      kj::ArrayPtr<const kj::ArrayPtr<const capnp::word>>) override { return kj::READY_NOW; }
  kj::Promise<void> writeMessages(
      kj::ArrayPtr<kj::ArrayPtr<const kj::ArrayPtr<const capnp::word>>>) override {
    return kj::READY_NOW;
  }
  kj::Promise<void> end() override { return kj::READY_NOW; }
};

class Message final: public capnp::OutgoingRpcMessage {
public:
  capnp::AnyPointer::Builder getBody() override { KJ_FAIL_REQUIRE("unused"); }
  size_t sizeInWords() override { return 16384 / 8; }
  void send() override {}
};

struct Flow {
  kj::Own<capnp::RpcFlowController> controller;
  std::vector<kj::Own<kj::PromiseFulfiller<void>>> acks;
  std::vector<std::unique_ptr<kj::Promise<void>>> credits;
  std::vector<unsigned> results;
  void send() {
    auto paf = kj::newPromiseAndFulfiller<void>();
    credits.emplace_back(new kj::Promise<void>(controller->send(
        kj::heap<Message>(), kj::mv(paf.promise))));
    acks.push_back(kj::mv(paf.fulfiller));
    results.push_back(2);
  }
  void poll(kj::WaitScope& wait) {
    for (size_t i=0; i<credits.size(); ++i) {
      if (credits[i] && credits[i]->poll(wait)) {
        try { credits[i]->wait(wait); results[i]=1; }
        catch (const kj::Exception&) { results[i]=3; }
        credits[i].reset();
      }
    }
  }
  unsigned result(size_t i) { return i<results.size() ? results[i] : 0; }
};

int main(int argc, char** argv) {
  KJ_REQUIRE(argc==2);
  std::ifstream input(argv[1]);
  KJ_REQUIRE(input.good());
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  struct Case {
    Stream stream;
    capnp::TwoPartyVatNetwork network{stream, capnp::rpc::twoparty::Side::SERVER};
    kj::Own<capnp::TwoPartyVatNetworkBase::Connection> connection;
    Flow flows[2];
    Case(kj::WaitScope& wait): connection(network.accept().wait(wait)) {}
  };
  std::unique_ptr<Case> current;
  std::string op;
  unsigned event;
  while(input >> op >> event) {
    if (op=="new") { current = std::make_unique<Case>(wait); continue; }
    KJ_REQUIRE(op=="step" && current);
    auto& c = *current;
    if (event==1 || event==2) {
      auto& flow = c.flows[event-1];
      if (!flow.controller.get()) flow.controller=c.connection->newStream();
      flow.send();
    } else if (event>=4 && event<=7) {
      auto& ack=c.flows[(event-4)/2].acks[0];
      if (event%2==0) ack->fulfill();
      else ack->reject(KJ_EXCEPTION(DISCONNECTED, "test ack failure"));
      ack=nullptr;
    } else if (event>=10 && event<=13) {
      c.stream.hint=event-10;
    } else { KJ_FAIL_REQUIRE("invalid event", event); }
    wait.poll();
    for(auto& flow:c.flows) flow.poll(wait);
    std::cout << c.stream.queries << ':' << c.flows[0].result(0) << ':'
              << c.flows[0].result(1) << ':' << c.flows[0].result(2) << ':'
              << c.flows[1].result(0) << ':' << c.flows[1].result(1) << '\n';
  }
}
