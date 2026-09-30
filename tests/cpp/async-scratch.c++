// Pinned C++ oracle for async caller-owned payload storage. MIT license.
#include <capnp/serialize-async.h>
#include <kj/async-io.h>
#include <kj/debug.h>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <vector>

class Input final: public kj::AsyncInputStream {
public:
  std::vector<unsigned char> bytes;
  size_t position = 0;
  kj::Promise<size_t> tryRead(void* output, size_t, size_t maximum) override {
    auto count = kj::min(maximum, bytes.size() - position);
    if (count > 0) memcpy(output, bytes.data() + position, count);
    position += count;
    return count;
  }
};

int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  std::ifstream cases(argv[1]);
  KJ_REQUIRE(cases.good());
  kj::EventLoop loop;
  kj::WaitScope wait(loop);
  size_t id, capacity;
  uint64_t limit;
  std::string hex;
  while (cases >> id >> capacity >> limit >> hex) {
    Input input;
    if (hex != "-") {
      for (size_t i = 0; i < hex.size(); i += 2) {
        input.bytes.push_back(static_cast<unsigned char>(std::stoul(hex.substr(i, 2), nullptr, 16)));
      }
    }
    auto scratch = kj::heapArray<capnp::word>(capacity);
    if (capacity > 0) memset(scratch.asBytes().begin(), 0xa5, scratch.asBytes().size());
    capnp::ReaderOptions options;
    options.traversalLimitInWords = limit;
    std::string state = "eof";
    bool borrowed = false;
    size_t used = 0;
    std::ostringstream segments;
    try {
      auto maybe = capnp::tryReadMessage(input, options, scratch.asPtr()).wait(wait);
      KJ_IF_SOME(message, maybe) {
        state = "ok";
        borrowed = message->getSegment(0).begin() == scratch.begin();
        uint32_t count = 1;
        for (size_t i = 0; i < 4; ++i) count += uint32_t(input.bytes[i]) << (8 * i);
        constexpr char DIGITS[] = "0123456789abcdef";
        for (uint32_t i = 0; i < count; ++i) {
          auto bytes = message->getSegment(i).asBytes();
          if (borrowed) used += bytes.size();
          segments << bytes.size() << ':';
          for (auto byte: bytes) segments << DIGITS[byte >> 4] << DIGITS[byte & 15];
          segments << ';';
        }
      }
    } catch (const kj::Exception&) {
      state = "error";
    }
    // Partial payload reads on error can overwrite a prefix of scratch, so
    // only successful reads and clean EOF make a tail-preservation claim.
    bool tail = true;
    if (state != "error") {
      auto bytes = scratch.asBytes();
      for (size_t i = used; i < bytes.size(); ++i) tail &= bytes[i] == 0xa5;
    }
    std::cout << id << ' ' << state << ' ' << input.position << ' '
              << borrowed << ' ' << tail << ' ' << segments.str() << '\n';
  }
}
