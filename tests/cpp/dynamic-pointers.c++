#include <capnp/dynamic.h>
#include <capnp/schema-loader.h>
#include <capnp/serialize.h>
#include <kj/debug.h>
#include <fcntl.h>
#include <unistd.h>
#include <iostream>
#include <string>

capnp::AnyPointer::Builder access(capnp::DynamicStruct::Builder root,
    const std::string& path, bool init) {
  auto dot = path.find('.');
  if (dot != std::string::npos) return access(root.get(path.substr(0, dot).c_str()).as<capnp::DynamicStruct>(), path.substr(dot + 1), init);
  return (init ? root.init(path.c_str()) : root.get(path.c_str())).as<capnp::AnyPointer>();
}
void edit(capnp::AnyStruct::Builder value) {
  value.getDataSection()[0] = 99;
  value.getPointerSection()[0].setAs<capnp::Text>("changed");
}
std::string apply(capnp::DynamicStruct::Builder root, const std::string& path,
    const std::string& kind, const std::string& action, unsigned encoding, unsigned count) {
  auto p = access(root, path, action == "init");
  if (kind == "any") {
    if (action == "edit") p.setAs<capnp::Text>("changed");
    switch (p.getPointerType()) {
      case capnp::PointerType::NULL_: return "null";
      case capnp::PointerType::STRUCT: return "struct";
      case capnp::PointerType::LIST: return "list";
      case capnp::PointerType::CAPABILITY: return "cap";
    }
  } else if (kind == "structure") {
    auto value = action == "init" ? p.initAsAnyStruct(count, count) : p.getAs<capnp::AnyStruct>();
    if (action == "edit") edit(value);
    return "struct " + std::to_string(value.getDataSection().size()) + " " + std::to_string(value.getPointerSection().size());
  } else {
    KJ_REQUIRE(kind == "list");
    if (action == "init") {
      if (encoding == 7) p.initAsListOfAnyStruct(2, 2, count);
      else p.initAsAnyList(static_cast<capnp::ElementSize>(encoding), count);
    }
    auto value = p.getAs<capnp::AnyList>();
    if (action == "edit") {
      switch (value.getElementSize()) {
        case capnp::ElementSize::FOUR_BYTES: value.as<capnp::List<uint32_t>>().set(0, 99); break;
        case capnp::ElementSize::POINTER: value.as<capnp::List<capnp::AnyPointer>>()[0].setAs<capnp::Text>("changed"); break;
        case capnp::ElementSize::INLINE_COMPOSITE: edit(value.as<capnp::List<capnp::AnyStruct>>()[0]); break;
        default: KJ_FAIL_REQUIRE("unexpected edit encoding");
      }
    }
    return "list " + std::to_string(static_cast<unsigned>(value.getElementSize())) + " " + std::to_string(value.size());
  }
  KJ_UNREACHABLE;
}
int main(int argc, char** argv) {
  try {
    KJ_REQUIRE(argc == 9);
    int schemaFd = open(argv[1], O_RDONLY); KJ_REQUIRE(schemaFd >= 0);
    capnp::StreamFdMessageReader request(schemaFd); capnp::SchemaLoader loader;
    for (auto node: request.getRoot<capnp::schema::CodeGeneratorRequest>().getNodes()) loader.load(node);
    auto schema = loader.get(std::stoull(argv[2])).asStruct();
    int seedFd = open(argv[3], O_RDONLY); KJ_REQUIRE(seedFd >= 0);
    capnp::StreamFdMessageReader seed(seedFd); capnp::MallocMessageBuilder message;
    message.setRoot(seed.getRoot<capnp::DynamicStruct>(schema)); auto root = message.getRoot<capnp::DynamicStruct>(schema);
    try { auto result = apply(root, argv[6], argv[5], argv[4], std::stoul(argv[7]), std::stoul(argv[8])); std::cout << "ok " << result << ' '; }
    catch (const kj::Exception&) { std::cout << "error "; }
    auto canonical = capnp::AnyStruct::Reader(root.asReader()).canonicalize(); const char* hex = "0123456789abcdef";
    for (auto b: canonical.asBytes()) std::cout << hex[b >> 4] << hex[b & 15]; std::cout << '\n';
    close(seedFd); close(schemaFd); return 0;
  } catch (const kj::Exception& e) { std::cerr << e.getDescription().cStr() << '\n'; return 2; }
}
