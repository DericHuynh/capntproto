#include <capnp/rpc.capnp.h>
#include <capnp/serialize-async.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <kj/io.h>
#include <sys/socket.h>
#include <unistd.h>
#include <cstring>
#include <fstream>
#include <iostream>
#include <iterator>
#include <vector>

kj::AutoCloseFd descriptor(unsigned char value) {
  char path[] = "/tmp/capntproto-buffered-fd-XXXXXX";
  int raw = mkstemp(path);
  KJ_REQUIRE(raw >= 0);
  kj::AutoCloseFd fd(raw);
  KJ_REQUIRE(unlink(path) == 0);
  KJ_REQUIRE(write(raw, &value, 1) == 1);
  return fd;
}

void send(int socket, const char* bytes, size_t size, std::vector<int> fds = {}) {
  iovec part{const_cast<char*>(bytes), size};
  alignas(cmsghdr) char ancillary[CMSG_SPACE(2 * sizeof(int))]{};
  msghdr msg{};
  msg.msg_iov = &part;
  msg.msg_iovlen = 1;
  if (!fds.empty()) {
    msg.msg_control = ancillary;
    msg.msg_controllen = CMSG_SPACE(fds.size() * sizeof(int));
    auto header = CMSG_FIRSTHDR(&msg);
    header->cmsg_level = SOL_SOCKET;
    header->cmsg_type = SCM_RIGHTS;
    header->cmsg_len = CMSG_LEN(fds.size() * sizeof(int));
    memcpy(CMSG_DATA(header), fds.data(), fds.size() * sizeof(int));
  }
  KJ_REQUIRE(sendmsg(socket, &msg, MSG_NOSIGNAL) == static_cast<ssize_t>(size));
}

int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2 || argc == 5);
  bool scratchMode = argc == 5;
  size_t capacity = scratchMode ? std::stoul(argv[2]) : 0;
  bool shortLived = !scratchMode || std::stoul(argv[3]) != 0;
  size_t bufferWords = scratchMode ? std::stoul(argv[4]) : 8192;
  std::ifstream file(argv[1], std::ios::binary);
  KJ_REQUIRE(file.good());
  std::vector<char> bytes(std::istreambuf_iterator<char>(file), {});
  KJ_REQUIRE(bytes.size() % 3 == 0);
  size_t frame = bytes.size() / 3;
  KJ_REQUIRE(frame > 8);
  auto io = kj::setupAsyncIo();
  for (size_t limit: {0, 1, 2}) {
    for (size_t prefix: {size_t(0), size_t(1), size_t(8), frame - 1}) {
      for (bool splitFds: {false, true}) {
        if (prefix == 0 && splitFds) continue;
        int sockets[2];
        KJ_REQUIRE(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, sockets) == 0);
        kj::AutoCloseFd sender(sockets[0]), receiver(sockets[1]);
        auto a = descriptor(11), x = descriptor(12), b = descriptor(22);
        send(sender, bytes.data(), frame);
        size_t count = prefix == 0 ? frame : prefix;
        send(sender, bytes.data() + frame, count,
            splitFds ? std::vector<int>{a} : std::vector<int>{a, x});
        if (prefix != 0) {
          send(sender, bytes.data() + frame + prefix, frame - prefix,
              splitFds ? std::vector<int>{x} : std::vector<int>{});
        }
        send(sender, bytes.data() + 2 * frame, frame, {b});
        KJ_REQUIRE(shutdown(sender, SHUT_WR) == 0);
        auto socket = io.lowLevelProvider->wrapUnixSocketFd(kj::mv(receiver));
        bool shared = false;
        capnp::BufferedMessageStream stream(*socket, [&](capnp::MessageReader&) {
          shared = shortLived;
          return shortLived;
        }, bufferWords);
        auto scratch = kj::heapArray<capnp::word>(capacity);
        auto fdSpace = kj::heapArray<kj::OwnFd>(limit);
        for (size_t n = 0; n < 3; ++n) {
          shared = false;
          if (capacity) memset(scratch.asBytes().begin(), 0xa5, scratch.asBytes().size());
          auto maybe = stream.tryReadMessage(fdSpace, capnp::ReaderOptions(), scratch).wait(io.waitScope);
          auto& result = KJ_REQUIRE_NONNULL(maybe);
          auto id = result.reader->getRoot<capnp::rpc::Message>().getFinish().getQuestionId();
          std::cout << limit << ' ' << prefix << ' ' << splitFds << ' ' << id << ' '
                    << shared << ' ' << result.fds.size();
          for (auto& fd: result.fds) {
            unsigned char value = 0;
            KJ_REQUIRE(pread(fd, &value, 1, 0) == 1);
            std::cout << ' ' << unsigned(value);
            fd = nullptr;
          }
          if (scratchMode) {
            // Fixtures use one segment; buffered scratch includes its table word.
            bool borrowed = capacity > 0 && result.reader->getSegment(0).begin() == scratch.begin() + 1;
            bool tail = true;
            size_t used = borrowed ? frame : 0;
            for (size_t i = used; i < scratch.asBytes().size(); ++i) {
              tail &= scratch.asBytes()[i] == 0xa5;
            }
            std::cout << ' ' << borrowed << ' ' << tail;
          }
          std::cout << '\n';
        }
        KJ_REQUIRE(stream.tryReadMessage(fdSpace).wait(io.waitScope) == kj::none);
      }
    }
  }
}
