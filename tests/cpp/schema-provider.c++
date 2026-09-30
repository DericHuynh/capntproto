// Qualify the custom SchemaFile API using an application-defined namespace.
#include <capnp/schema-parser.h>
#include <capnp/dynamic.h>
#include <capnp/message.h>
#include <kj/debug.h>
#include <fstream>
#include <iostream>
#include <map>
#include <string>

struct Store {
  std::map<std::string, std::string> bytes;
  mutable std::map<std::string, unsigned> reads;
};

class File final: public capnp::SchemaFile {
public:
  File(Store& store, std::string key): store(store), key(kj::mv(key)) {}
  kj::StringPtr getDisplayName() const override {
    if (key == "main") return "main.capnp";
    if (key == "types") return "types.capnp";
    return "blob.bin";
  }
  kj::Array<const char> readContent() const override {
    ++store.reads[key];
    auto& bytes = store.bytes.at(key);
    return kj::heapArray(bytes.data(), bytes.size());
  }
  kj::Maybe<kj::Own<capnp::SchemaFile>> import(kj::StringPtr path) const override {
    if (key == "main" && (path == "pkg:types" || path == "/alias//../types"))
      return kj::heap<File>(store, "types");
    if (key == "types" && path == "root:alias") return kj::heap<File>(store, "main");
    if (key == "main" && path == "asset:blob") return kj::heap<File>(store, "blob");
    KJ_REQUIRE(path != "never:open", "unused import must remain lazy");
    return kj::none;
  }
  bool operator==(const capnp::SchemaFile& other) const override {
    auto* file = dynamic_cast<const File*>(&other);
    return file != nullptr && &store == &file->store && key == file->key;
  }
  size_t hashCode() const override { return std::hash<std::string>()(key); }
  void reportError(SourcePos start, SourcePos, kj::StringPtr message) const override {
    KJ_FAIL_REQUIRE("schema callback error", key.c_str(), start.byte, message);
  }
private:
  Store& store;
  std::string key;
};

std::string hex(kj::ArrayPtr<const kj::byte> bytes) {
  const char* digits = "0123456789abcdef";
  std::string out;
  for (auto byte: bytes) { out += digits[byte >> 4]; out += digits[byte & 15]; }
  return out;
}

void dump(capnp::SchemaParser& parser, capnp::ParsedSchema schema) {
  auto node = capnp::AnyStruct::Reader(schema.getProto()).canonicalize();
  std::cout << "node " << schema.getProto().getId() << ' ' << hex(node.asBytes()) << ' ';
  KJ_IF_SOME(source, parser.getSourceInfo(schema)) {
    auto info = capnp::AnyStruct::Reader(source).canonicalize();
    std::cout << hex(info.asBytes());
  } else {
    std::cout << '-';
  }
  std::cout << '\n';
}

int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 2);
  Store store;
  for (auto key: {"main", "types"}) {
    std::ifstream input(std::string(argv[1]) + "/provider-" + key + ".capnp", std::ios::binary);
    KJ_REQUIRE(input.good());
    store.bytes[key] = std::string(std::istreambuf_iterator<char>(input), {});
  }
  store.bytes["blob"] = std::string("\0\xff*", 3);
  capnp::SchemaParser parser;
  auto main = parser.parseFile(kj::heap<File>(store, "main"));
  auto again = parser.parseFile(kj::heap<File>(store, "main"));
  KJ_REQUIRE(main == again);
  auto root = main.getNested("Root");
  auto blob = main.getNested("blob");
  auto types = main.getNested("A");
  KJ_REQUIRE(types == main.getNested("B"));
  auto item = types.getNested("Item");
  auto later = types.getNested("Later");
  for (auto schema: {main, root, blob, types, item, later}) dump(parser, schema);
  capnp::MallocMessageBuilder message;
  auto value = message.initRoot<capnp::DynamicStruct>(root.asStruct());
  KJ_REQUIRE(value.asReader().get("payload").as<capnp::Data>().size() == 3);
  value.init("a").as<capnp::DynamicStruct>().set("value", uint32_t(71));
  auto wire = capnp::AnyStruct::Reader(value.asReader()).canonicalize();
  std::cout << "wire " << hex(wire.asBytes()) << '\n';
  KJ_REQUIRE(store.reads["main"] == 1 && store.reads["types"] == 1);
  // C++ may read an embed per use; compare its bytes, not callback invocation counts.
}
