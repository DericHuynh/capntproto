#include <capnp/dynamic.h>
#include <capnp/schema-loader.h>
#include <capnp/serialize.h>
#include <kj/debug.h>
#include <fcntl.h>
#include <unistd.h>
#include <fstream>
#include <iostream>
#include <string>

void apply(capnp::DynamicStruct::Builder root, const std::string& action,
           const std::string& path) {
  auto dot = path.find('.');
  if (dot != std::string::npos) {
    apply(root.get(path.substr(0, dot).c_str()).as<capnp::DynamicStruct>(), action,
          path.substr(dot + 1));
  } else if (action == "init") {
    root.init(path.c_str());
  } else if (action == "clear") {
    root.clear(path.c_str());
  } else if (action == "get-struct") {
    root.get(path.c_str()).as<capnp::DynamicStruct>();
  } else if (action == "get-list") {
    root.get(path.c_str()).as<capnp::DynamicList>();
  } else {
    KJ_REQUIRE(action == "get");
    root.get(path.c_str());
  }
}

int main(int argc, char** argv) {
  try {
    KJ_REQUIRE(argc == 5 || (argc == 6 && std::string(argv[5]) == "--report-errors"));
    int schemaFd = open(argv[1], O_RDONLY); KJ_REQUIRE(schemaFd >= 0);
    capnp::StreamFdMessageReader request(schemaFd);
    capnp::SchemaLoader loader;
    for (auto node: request.getRoot<capnp::schema::CodeGeneratorRequest>().getNodes()) {
      loader.load(node);
    }
    auto schema = loader.get(std::stoull(argv[2])).asStruct();
    int seedFd = open(argv[3], O_RDONLY); KJ_REQUIRE(seedFd >= 0);
    capnp::StreamFdMessageReader seed(seedFd);
    capnp::MallocMessageBuilder message;
    message.setRoot(seed.getRoot<capnp::DynamicStruct>(schema));
    auto root = message.getRoot<capnp::DynamicStruct>(schema);
    std::ifstream input(argv[4]); KJ_REQUIRE(input.good());
    std::string action, path;
    const char* hex = "0123456789abcdef";
    while (input >> action >> path) {
      if (argc == 6) {
        try { apply(root, action, path); std::cout << "ok "; }
        catch (const kj::Exception&) { std::cout << "error "; }
      } else {
        apply(root, action, path);
      }
      auto canonical = capnp::AnyStruct::Reader(root.asReader()).canonicalize();
      for (auto byte: canonical.asBytes()) std::cout << hex[byte >> 4] << hex[byte & 15];
      std::cout << '\n';
    }
    close(seedFd);
    close(schemaFd);
    return 0;
  } catch (const kj::Exception& e) {
    std::cerr << e.getDescription().cStr() << '\n';
    return 2;
  }
}
