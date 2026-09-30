#include <capnp/dynamic.h>
#include <capnp/schema-loader.h>
#include <capnp/serialize.h>
#include <kj/debug.h>
#include <fcntl.h>
#include <unistd.h>
#include <iostream>
#include <string>

void observe(capnp::DynamicStruct::Reader reader, unsigned index, std::string prefix) {
  for (auto field: reader.getSchema().getFields()) {
    auto name = prefix + field.getProto().getName().cStr();
    bool a = reader.has(field, capnp::HasMode::NON_NULL);
    bool b = reader.has(field, capnp::HasMode::NON_DEFAULT);
    std::cout << index << ' ' << name << ' ' << a << ' ' << b << '\n';
    if (a && field.getProto().isGroup()) {
      observe(reader.get(field).as<capnp::DynamicStruct>(), index, name + ".");
    }
  }
}
int main(int argc, char** argv) {
  KJ_REQUIRE(argc == 5);
  int schemaFd = open(argv[1], O_RDONLY);
  int dataFd = open(argv[2], O_RDONLY);
  KJ_REQUIRE(schemaFd >= 0 && dataFd >= 0);
  capnp::StreamFdMessageReader request(schemaFd);
  capnp::SchemaLoader loader;
  for (auto node: request.getRoot<capnp::schema::CodeGeneratorRequest>().getNodes()) loader.load(node);
  auto schema = loader.get(std::stoull(argv[3])).asStruct();
  for (unsigned i = 0; i < std::stoul(argv[4]); ++i) {
    capnp::StreamFdMessageReader message(dataFd);
    observe(message.getRoot<capnp::DynamicStruct>(schema), i, "");
  }
  close(dataFd);
  close(schemaFd);
}
