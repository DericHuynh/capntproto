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
  return out.empty() ? "-" : out;
}

int main() {
  auto fs = kj::newDiskFilesystem();
  capnp::SchemaParser parser;
  auto file = parser.parseFromDirectory(fs->getCurrent(), kj::Path::parse("reflection.capnp"), nullptr);
  auto common = parser.parseFromDirectory(fs->getCurrent(), kj::Path::parse("reflection-common.capnp"), nullptr);
  std::map<uint64_t, capnp::Schema> schemas;
  for (auto schema: parser.getAllLoaded()) schemas.emplace(schema.getProto().getId(), schema);
  for (auto [id, schema]: schemas) {
    auto proto = schema.getProto();
    auto info = KJ_ASSERT_NONNULL(parser.getSourceInfo(schema));
    std::cout << "node " << id << ' ' << unsigned(proto.which()) << ' '
        << hex(proto.getDisplayName().asBytes()) << ' ' << proto.getStartByte() << ' ' << proto.getEndByte()
        << ' ' << info.getStartByte() << ' ' << info.getEndByte() << ' ' << hex(info.getDocComment().asBytes()) << '\n';
    for (auto child: proto.getNestedNodes())
      std::cout << "child " << id << ' ' << child.getId() << ' ' << hex(child.getName().asBytes()) << '\n';
    unsigned index = 0;
    for (auto member: info.getMembers())
      std::cout << "member " << id << ' ' << index++ << ' ' << member.getStartByte() << ' '
          << member.getEndByte() << ' ' << hex(member.getDocComment().asBytes()) << '\n';
  }
  // Exercise the actual ParsedSchema navigation surface, not just wire nodes.
  for (auto parent: {file, common, file.getNested("Record"), common.getNested("Box")}) {
    KJ_REQUIRE(parent.findNested("Missing") == kj::none);
    for (auto child: parent.getAllNested()) {
      auto name = child.getUnqualifiedName();
      KJ_REQUIRE(parent.getNested(name) == child);
      KJ_REQUIRE(KJ_ASSERT_NONNULL(parent.findNested(name)) == child);
      KJ_REQUIRE(child.getSourceInfo().getId() == child.getProto().getId());
      std::cout << "lookup " << parent.getProto().getId() << ' ' << hex(name.asBytes()) << ' ' << child.getProto().getId() << '\n';
    }
  }
  auto record = file.getNested("Record").asStruct();
  KJ_REQUIRE(record.getFieldByName("box").getType().asStruct().getFieldByName("value").getType().isText());
  capnp::MallocMessageBuilder message;
  auto value = message.initRoot<capnp::DynamicStruct>(record);
  // Read through a reader: a builder getter would materialize the Text default.
  KJ_REQUIRE(value.asReader().get("count").as<uint32_t>() == 17);
  KJ_REQUIRE(value.asReader().get("label").as<capnp::Text>() == "embedded label\n");
  value.set("count", uint32_t(99));
  value.init("box").as<capnp::DynamicStruct>().set("value", "boxed");
  value.init("details").as<capnp::DynamicStruct>().set("active", false);
  value.init("child").as<capnp::DynamicStruct>().set("label", "child");
  auto entries = value.init("entries", 2).as<capnp::DynamicList>();
  KJ_REQUIRE(entries[0].as<capnp::DynamicStruct>().get("value").as<int16_t>() == -7);
  entries[1].as<capnp::DynamicStruct>().set("value", int16_t(123));
  auto canonical = capnp::AnyStruct::Reader(value.asReader()).canonicalize();
  std::cout << "wire " << hex(canonical.asBytes()) << '\n';
}
