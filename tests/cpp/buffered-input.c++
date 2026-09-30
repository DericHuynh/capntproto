#include <capnp/rpc.h>
#include <capnp/rpc.capnp.h>
#include <capnp/serialize-async.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <cstring>
#include <fstream>
#include <iostream>
#include <iterator>
#include <vector>

class Input final: public kj::AsyncIoStream {
public:
  std::vector<char> bytes;
  size_t position = 0, calls = 0;
  kj::Promise<size_t> tryRead(void* out, size_t, size_t max) override {
    ++calls;
    auto count = kj::min(max, bytes.size() - position);
    memcpy(out, bytes.data() + position, count);
    position += count;
    return count;
  }
  kj::Promise<void> write(kj::ArrayPtr<const kj::byte>) override { KJ_FAIL_REQUIRE("unused write"); }
  kj::Promise<void> write(kj::ArrayPtr<const kj::ArrayPtr<const kj::byte>>) override { KJ_FAIL_REQUIRE("unused write"); }
  kj::Promise<void> whenWriteDisconnected() override { return kj::NEVER_DONE; }
  void shutdownWrite() override {}
};
int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  std::ifstream file(argv[1], std::ios::binary);
  KJ_REQUIRE(file.good());
  Input input;
  input.bytes = std::vector<char>(std::istreambuf_iterator<char>(file), {});
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  bool shared = false;
  capnp::BufferedMessageStream stream(input, [&](capnp::MessageReader& msg) {
    shared = capnp::IncomingRpcMessage::isShortLivedRpcMessage(msg.getRoot<capnp::AnyPointer>());
    return shared;
  }, 256);
  std::vector<kj::Own<capnp::MessageReader>> retained;
  std::vector<uint16_t> retainedKinds;
  for (;;) {
    shared = false;
    auto maybe = stream.tryReadMessage().wait(wait);
    KJ_IF_SOME(msg, maybe) {
      auto kind = static_cast<uint16_t>(msg->getRoot<capnp::rpc::Message>().which());
      auto shortLived = capnp::IncomingRpcMessage::isShortLivedRpcMessage(msg->getRoot<capnp::AnyPointer>());
      std::cout << kind << ' ' << shortLived << ' ' << shared << ' ' << input.calls << '\n';
      if (shared) {
        bool rejected = false;
        KJ_IF_SOME(error, kj::runCatchingExceptions([&]() { stream.tryReadMessage().wait(wait); })) {
          rejected = true;
        }
        KJ_REQUIRE(rejected);
      } else {
        retainedKinds.push_back(kind);
        retained.push_back(kj::mv(msg));
      }
    } else { break; }
  }
  for (size_t i = 0; i < retained.size(); ++i) {
    KJ_REQUIRE(static_cast<uint16_t>(retained[i]->getRoot<capnp::rpc::Message>().which()) == retainedKinds[i]);
  }
}
