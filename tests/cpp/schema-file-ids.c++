#include <capnp/schema-parser.h>
#include <capnp/dynamic.h>
#include <capnp/message.h>
#include <kj/debug.h>
#include <kj/filesystem.h>
#include <iostream>
#include <map>
#include <string>

std::string hex(kj::ArrayPtr<const kj::byte> bytes) {
  const char* digits = "0123456789abcdef";
  std::string out;
  for (auto byte: bytes) { out += digits[byte >> 4]; out += digits[byte & 15]; }
  return out;
}

int main() {
  auto fs = kj::newDiskFilesystem();
  bool rejected = false;
  try {
    capnp::SchemaParser strict;
    strict.parseFromDirectory(fs->getCurrent(), kj::Path::parse("auto-id-main.capnp"), nullptr);
  } catch (kj::Exception&) { rejected = true; }
  KJ_REQUIRE(rejected);

  capnp::SchemaParser parser;
  parser.setFileIdsRequired(false);
  auto main = parser.parseFromDirectory(fs->getCurrent(), kj::Path::parse("auto-id-main.capnp"), nullptr);
  auto types = parser.parseFromDirectory(fs->getCurrent(), kj::Path::parse("auto-id-types.capnp"), nullptr);
  auto mainId = main.getProto().getId();
  auto typesId = types.getProto().getId();
  KJ_REQUIRE((mainId >> 63) != 0 && (typesId >> 63) != 0 && mainId != typesId);
  KJ_REQUIRE(main.getNested("I") == types && types.getNested("Root") == main);
  KJ_REQUIRE(main.getNested("Fixed").getProto().getId() == 0xedddddddddddddddull);
  KJ_REQUIRE(parser.parseFromDirectory(fs->getCurrent(), kj::Path::parse("auto-id-main.capnp"), nullptr) == main);

  capnp::SchemaParser fresh;
  fresh.setFileIdsRequired(false);
  auto different = fresh.parseFromDirectory(fs->getCurrent(), kj::Path::parse("auto-id-main.capnp"), nullptr);
  KJ_REQUIRE(different.getProto().getId() != mainId);
  KJ_REQUIRE(different.getNested("Config").getProto().getId() != main.getNested("Config").getProto().getId());
  KJ_REQUIRE(different.getNested("Fixed").getProto().getId() == main.getNested("Fixed").getProto().getId());
  std::cout << "ids " << mainId << ' ' << typesId << '\n';

  std::map<uint64_t, capnp::Schema> schemas;
  for (auto schema: parser.getAllLoaded()) schemas.emplace(schema.getProto().getId(), schema);
  for (auto [id, schema]: schemas) {
    auto node = capnp::AnyStruct::Reader(schema.getProto()).canonicalize();
    auto info = capnp::AnyStruct::Reader(KJ_ASSERT_NONNULL(parser.getSourceInfo(schema))).canonicalize();
    std::cout << "node " << id << ' ' << hex(node.asBytes()) << ' ' << hex(info.asBytes()) << '\n';
  }
  capnp::MallocMessageBuilder message;
  auto value = message.initRoot<capnp::DynamicStruct>(main.getNested("Config").asStruct());
  value.init("server").as<capnp::DynamicStruct>().set("port", uint16_t(9000));
  value.init("details").as<capnp::DynamicStruct>().set("enabled", false);
  value.set("label", "configured");
  auto wire = capnp::AnyStruct::Reader(value.asReader()).canonicalize();
  std::cout << "wire " << hex(wire.asBytes()) << '\n';
}
