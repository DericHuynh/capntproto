#include "runtime-test.capnp.h"
#include <capnp/rpc.h>
#include <capnp/rpc-twoparty.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <kj/io.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <cstring>
#include <cstdlib>

class FileServer final: public Harness::Server {
public:
  explicit FileServer(unsigned char value) {
    char path[] = "/tmp/reproto-cpp-fd-XXXXXX";
    int raw = mkstemp(path);
    KJ_REQUIRE(raw >= 0);
    fd = kj::AutoCloseFd(raw);
    KJ_REQUIRE(unlink(path) == 0);
    KJ_REQUIRE(write(raw, &value, 1) == 1);
  }
  kj::Maybe<int> getFd() override { return fd.get(); }
private:
  kj::AutoCloseFd fd;
  kj::Promise<void> echo(EchoContext context) override {
    context.getResults().setValue(42);
    return kj::READY_NOW;
  }
  kj::Promise<void> bounce(BounceContext context) override {
    auto cap = context.getParams().getCap();
    auto available = cap.getFd();
    return available.then([cap = kj::mv(cap), context = kj::mv(context)](kj::Maybe<int> received) mutable {
      int raw = KJ_REQUIRE_NONNULL(received);
      unsigned char value = 0;
      KJ_REQUIRE(pread(raw, &value, 1, 0) == 1 && value == 7);
      context.getResults().setCap(kj::heap<FileServer>(10));
    });
  }
  kj::Promise<void> fdCaps(FdCapsContext context) override {
    context.getResults().setCaps(context.getParams().getCaps());
    return kj::READY_NOW;
  }
};
int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  int raw = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  KJ_REQUIRE(raw >= 0);
  kj::AutoCloseFd socketFd(raw);
  sockaddr_un address{};
  address.sun_family = AF_UNIX;
  KJ_REQUIRE(std::strlen(argv[1]) < sizeof(address.sun_path));
  std::strcpy(address.sun_path, argv[1]);
  KJ_REQUIRE(connect(raw, reinterpret_cast<sockaddr*>(&address), sizeof(address)) == 0);
  auto io = kj::setupAsyncIo();
  auto stream = io.lowLevelProvider->wrapUnixSocketFd(kj::mv(socketFd));
  capnp::TwoPartyVatNetwork network(*stream, 16, capnp::rpc::twoparty::Side::SERVER);
  auto server = capnp::makeRpcServer(network, kj::heap<FileServer>(9));
  network.onDisconnect().wait(io.waitScope);
}
