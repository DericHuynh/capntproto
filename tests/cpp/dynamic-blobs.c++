#include <capnp/dynamic.h>
#include <capnp/schema-loader.h>
#include <capnp/serialize.h>
#include <kj/debug.h>
#include <fcntl.h>
#include <unistd.h>
#include <iostream>
#include <string>

capnp::DynamicValue::Builder access(capnp::DynamicStruct::Builder root,
    const std::string& path, const std::string& action, unsigned size) {
  auto dot = path.find('.');
  if (dot != std::string::npos) {
    return access(root.get(path.substr(0, dot).c_str()).as<capnp::DynamicStruct>(),
        path.substr(dot + 1), action, size);
  }
  auto slash = path.find('/');
  if (slash != std::string::npos) {
    auto list = root.get(path.substr(0, slash).c_str()).as<capnp::DynamicList>();
    auto index = std::stoul(path.substr(slash + 1));
    return action == "init" ? list.init(index, size) : list[index];
  }
  return action == "init" ? root.init(path.c_str(), size) : root.get(path.c_str());
}
void hex(kj::ArrayPtr<const kj::byte> bytes) {
  const char* digits = "0123456789abcdef";
  for (auto b: bytes) std::cout << digits[b >> 4] << digits[b & 15];
}
int main(int argc, char** argv) {
  try {
    KJ_REQUIRE(argc == 8);
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
    try {
      auto value = access(root, argv[6], argv[4], std::stoul(argv[7]));
      kj::ArrayPtr<kj::byte> bytes;
      if (std::string(argv[5]) == "text") bytes = value.as<capnp::Text>().asBytes();
      else bytes = value.as<capnp::Data>();
      if (std::string(argv[4]) == "edit" && bytes.size() > 0) bytes[0] = 'Z';
      std::cout << "ok " << bytes.size() << ':'; hex(bytes); std::cout << ' ';
    } catch (const kj::Exception&) { std::cout << "error "; }
    auto canonical = capnp::AnyStruct::Reader(root.asReader()).canonicalize();
    hex(canonical.asBytes()); std::cout << '\n';
    close(seedFd); close(schemaFd);
    return 0;
  } catch (const kj::Exception& e) { std::cerr << e.getDescription().cStr() << '\n'; return 2; }
}
