#include <capnp/dynamic.h>
#include <capnp/schema-loader.h>
#include <capnp/serialize.h>
#include <kj/debug.h>
#include <fcntl.h>
#include <unistd.h>
#include <iostream>
#include <sstream>
#include <string>

unsigned apply(capnp::DynamicStruct::Builder root, const std::string& action,
               const std::string& path) {
  std::istringstream parts(path);
  std::string part;
  std::getline(parts, part, '/');
  auto list = root.get(part.c_str()).as<capnp::DynamicList>();
  while (std::getline(parts, part, '/')) {
    list = list[std::stoul(part)].as<capnp::DynamicList>();
  }
  unsigned size = list.size();
  if (action == "number") list.set(0, uint32_t(99));
  else if (action == "flag") list.set(0, true);
  else if (action == "enum") list.set(0, capnp::DynamicEnum(list.getSchema().getEnumElementType(), 1));
  else if (action == "text") list.set(0, capnp::Text::Reader("updated"));
  else if (action == "data") list.set(0, capnp::Data::Reader(reinterpret_cast<const kj::byte*>("updated"), 7));
  else if (action == "void") list.set(0, capnp::VOID);
  else if (action == "record") list[0].as<capnp::DynamicStruct>().set("payload", capnp::Text::Reader("updated"));
  else KJ_REQUIRE(action == "get");
  return size;
}

int main(int argc, char** argv) {
  try {
    KJ_REQUIRE(argc == 6);
    int schemaFd = open(argv[1], O_RDONLY); KJ_REQUIRE(schemaFd >= 0);
    capnp::StreamFdMessageReader request(schemaFd);
    capnp::SchemaLoader loader;
    for (auto node: request.getRoot<capnp::schema::CodeGeneratorRequest>().getNodes()) loader.load(node);
    auto schema = loader.get(std::stoull(argv[2])).asStruct();
    int seedFd = open(argv[3], O_RDONLY); KJ_REQUIRE(seedFd >= 0);
    capnp::StreamFdMessageReader seed(seedFd);
    capnp::MallocMessageBuilder message;
    message.setRoot(seed.getRoot<capnp::DynamicStruct>(schema));
    auto root = message.getRoot<capnp::DynamicStruct>(schema);
    try { auto size = apply(root, argv[4], argv[5]); std::cout << "ok " << size << ' '; }
    catch (const kj::Exception&) { std::cout << "error "; }
    const char* hex = "0123456789abcdef";
    auto canonical = capnp::AnyStruct::Reader(root.asReader()).canonicalize();
    for (auto byte: canonical.asBytes()) std::cout << hex[byte >> 4] << hex[byte & 15];
    std::cout << '\n';
    close(seedFd); close(schemaFd);
    return 0;
  } catch (const kj::Exception& e) {
    std::cerr << e.getDescription().cStr() << '\n';
    return 2;
  }
}
